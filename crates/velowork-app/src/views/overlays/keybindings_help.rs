use crate::keybindings::{
    format_keystroke, get_action_descriptions, get_config,
    keystroke_to_config_string, reset_to_defaults, update_config,
    Cancel, ConflictKind, KeybindingConfig, KeybindingEntry, ShowKeybindings,
};
use crate::theme::{surface_bg_t, theme};
use crate::views::components::{modal_content, modal_header};
use velowork_ui::badge::keyboard_hint;
use crate::ui::tokens::{
    ui_text_lg, ui_text_md, ui_text_ms, ui_text_sm, ui_text_xl, RADIUS_MD, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS,
};
use gpui::prelude::*;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::empty_state::empty_state;
use velowork_ui::input::{InputChangedEvent, InputState};
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::{h_flex, v_flex, tab_active_indicator, tab_style, Tooltip};

const CATEGORIES: &[&str] = &[
    "Global",
    "Terminal",
    "Navigation",
    "View",
    "Search",
    "Fullscreen",
    "Project",
    "Other",
];

/// Helper to normalize raw category strings into standard tabs.
fn normalize_category(raw_category: &str) -> &'static str {
    match raw_category {
        "Global" | "Window" | "Help" | "Settings" | "Update" | "System" => "Global",
        "Terminal" | "Layout" => "Terminal",
        "Navigation" => "Navigation",
        "View" | "Panel" | "Services" | "Git" => "View",
        "Search" => "Search",
        "Fullscreen" => "Fullscreen",
        "Project" | "Session" => "Project",
        _ => "Other",
    }
}

/// Helper to check if a specific keybinding entry is customized relative to defaults.
fn is_entry_customized(
    action: &str,
    entry_index: usize,
    entry: &KeybindingEntry,
    defaults: &KeybindingConfig,
) -> bool {
    if let Some(default_entries) = defaults.bindings.get(action) {
        if let Some(default_entry) = default_entries.get(entry_index) {
            return entry != default_entry;
        }
    }
    true
}

/// Helper to check if an entry belongs to the current platform.
/// Standard default macOS entries (`cmd-...`) are hidden on Linux/Windows.
/// Standard default Linux/Windows entries (`ctrl-...` without `cmd-...`) are hidden on macOS.
/// Any customized entry (modified by the user on this platform) is always shown.
fn is_entry_matching_platform(
    entry: &KeybindingEntry,
    is_custom: bool,
) -> bool {
    if is_custom || entry.keystroke == "unset" || entry.keystroke.is_empty() {
        return true;
    }
    let ks = entry.keystroke.to_lowercase();
    if cfg!(target_os = "macos") {
        if ks.contains("ctrl-") && !ks.contains("cmd-") {
            return false;
        }
    } else if ks.contains("cmd-") {
        return false;
    }
    true
}

/// Helper to get i18n display name for categories.
fn category_i18n(category: &str, cx: &App) -> String {
    match category {
        "All" => i18n!(cx, "keybindings.tab_all"),
        "Global" => i18n!(cx, "keybindings.tab_global"),
        "Terminal" => i18n!(cx, "keybindings.tab_terminal"),
        "Navigation" => i18n!(cx, "keybindings.tab_navigation"),
        "View" => i18n!(cx, "keybindings.tab_view"),
        "Search" => i18n!(cx, "keybindings.tab_search"),
        "Fullscreen" => i18n!(cx, "keybindings.tab_fullscreen"),
        "Project" => i18n!(cx, "keybindings.tab_project"),
        "Other" => i18n!(cx, "keybindings.tab_other"),
        other => other.to_string(),
    }
}

/// Safely translate action name without leaking raw i18n keys
fn translate_action_name(raw_name: &str, action_key: &str, cx: &App) -> String {
    let key1 = format!("commands.{}", raw_name);
    let trans1 = i18n!(cx, key1.as_str());
    if trans1 != key1 && !trans1.is_empty() {
        return trans1;
    }
    let key2 = format!("commands.{}", action_key);
    let trans2 = i18n!(cx, key2.as_str());
    if trans2 != key2 && !trans2.is_empty() {
        return trans2;
    }
    raw_name.to_string()
}

/// Safely translate action description without leaking raw i18n keys
fn translate_action_desc(raw_desc: &str, cx: &App) -> String {
    if raw_desc.is_empty() {
        return String::new();
    }
    let key = format!("commands.{}", raw_desc);
    let trans = i18n!(cx, key.as_str());
    if trans != key && !trans.is_empty() {
        trans
    } else {
        raw_desc.to_string()
    }
}

/// State for the keybinding currently being recorded
#[derive(Clone, Debug)]
struct EditingState {
    /// The action being edited
    action: String,
    /// Index of the binding entry within the action's entries
    entry_index: usize,
    /// First chord keystroke if recording a chord sequence
    first_chord: Option<String>,
    /// Whether we're waiting for a potential second chord keystroke
    waiting_for_chord: bool,
}

/// Flattened representation of a keybinding entry row for display and keyboard navigation
#[derive(Clone, Debug)]
struct DisplayRow {
    action: String,
    entry_index: usize,
    keystroke: String,
    context: Option<String>,
    is_customized: bool,
    is_enabled: bool,
    action_name: String,
    action_description: String,
}

/// Keybindings help overlay with inline editing, dynamic category tabs, and search
pub struct KeybindingsHelp {
    focus_handle: FocusHandle,
    show_reset_confirmation: bool,
    /// Current editing/recording state
    editing: Option<EditingState>,
    /// Timer handle for chord timeout
    _chord_timer: Option<async_channel::Sender<()>>,
    /// Conflict warning after recording or editing
    pending_conflict: Option<String>,
    /// Keystroke interceptor subscription (active during recording)
    _interceptor: Option<Subscription>,
    /// Real, IME-capable search input for filtering keybindings.
    search_input: Option<Entity<InputState>>,
    /// Cached search query, kept in sync with `search_input` via subscription.
    search_query: String,
    /// Selected category Tab (`None` for "All")
    selected_tab: Option<&'static str>,
    /// Currently highlighted item index in the filtered list
    selected_index: usize,
    /// Scroll handle for vertical list
    scroll_handle: ScrollHandle,
    initial_focus_done: bool,
}

impl KeybindingsHelp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            show_reset_confirmation: false,
            editing: None,
            _chord_timer: None,
            pending_conflict: None,
            _interceptor: None,
            search_input: None,
            search_query: String::new(),
            selected_tab: None,
            selected_index: 0,
            scroll_handle: ScrollHandle::new(),
            initial_focus_done: false,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(KeybindingsHelpEvent::Close);
    }

    fn handle_reset_to_defaults(&mut self, cx: &mut Context<Self>) {
        if self.show_reset_confirmation {
            if let Err(e) = reset_to_defaults() {
                log::error!("Failed to reset keybindings: {}", e);
            }
            self.show_reset_confirmation = false;
            self.pending_conflict = None;
            cx.emit(KeybindingsHelpEvent::ReloadBindings);
            cx.notify();
        } else {
            self.show_reset_confirmation = true;
            cx.notify();
        }
    }

    fn cancel_reset(&mut self, cx: &mut Context<Self>) {
        self.show_reset_confirmation = false;
        cx.notify();
    }

    /// Start recording a keystroke for a specific binding
    fn start_recording(&mut self, action: String, entry_index: usize, cx: &mut Context<Self>) {
        self.editing = Some(EditingState {
            action,
            entry_index,
            first_chord: None,
            waiting_for_chord: false,
        });
        self.pending_conflict = None;
        self._chord_timer = None;

        // Install a global keystroke interceptor that fires BEFORE action dispatch.
        // This prevents recorded keystrokes from triggering their bound actions.
        let this = cx.entity().downgrade();
        self._interceptor = Some(cx.intercept_keystrokes(move |event, window, cx| {
            let keystroke = &event.keystroke;

            // Escape cancels recording instead of being recorded
            if keystroke.key == "escape" && !keystroke.modifiers.modified() {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.cancel_recording(cx);
                    });
                }
                cx.stop_propagation();
                return;
            }

            let keystroke = keystroke.clone();
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    this.handle_recorded_keystroke(&keystroke, window, cx);
                });
            }
            cx.stop_propagation();
        }));

        cx.notify();
    }

    /// Cancel the current recording
    fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        self.editing = None;
        self._chord_timer = None;
        self._interceptor = None;
        self.pending_conflict = None;
        cx.notify();
    }

    /// Handle a keystroke during recording
    fn handle_recorded_keystroke(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };

        // Ignore modifier-only keypresses
        let key = keystroke.key.as_str();
        if matches!(key, "shift" | "control" | "alt" | "platform" | "function" | "") {
            return;
        }

        let config_str = keystroke_to_config_string(keystroke);

        if editing.waiting_for_chord {
            // This is the second keystroke of a chord
            let Some(first) = editing.first_chord.take() else {
                return;
            };
            let chord = format!("{} {}", first, config_str);
            self.finalize_recording(chord, window, cx);
        } else {
            // First keystroke — start chord timer
            editing.first_chord = Some(config_str);
            editing.waiting_for_chord = true;

            // Start a 1-second timer for chord completion
            let (cancel_tx, cancel_rx) = async_channel::bounded::<()>(1);
            self._chord_timer = Some(cancel_tx);

            cx.spawn_in(window, async move |this, cx| {
                let timeout = smol::Timer::after(std::time::Duration::from_secs(1));
                smol::future::or(async { timeout.await; true }, async { let _ = cancel_rx.recv().await; false }).await;

                // If we get here and still waiting, finalize with single keystroke
                let _ = cx.update(|window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(editing) = this.editing.as_mut()
                            && editing.waiting_for_chord
                            && let Some(single) = editing.first_chord.take()
                        {
                            this.finalize_recording(single, window, cx);
                        }
                    });
                });
            }).detach();

            cx.notify();
        }
    }

    /// Finalize recording: validate conflict, save the new keystroke, and reload
    fn finalize_recording(&mut self, new_keystroke: String, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.take() else {
            return;
        };
        self._chord_timer = None;
        self._interceptor = None;

        // Perform conflict check before updating config
        let context = {
            let config = get_config();
            config.bindings.get(&editing.action)
                .and_then(|entries| entries.get(editing.entry_index))
                .and_then(|e| e.context.clone())
        };

        let descriptions = get_action_descriptions();
        if let Some(conflict) = get_config().check_conflict(&editing.action, &new_keystroke, context.as_deref()) {
            let other_action = conflict.action1;
            let other_name = descriptions
                .get(other_action.as_str())
                .map(|d| d.name)
                .unwrap_or(other_action.as_str());
            let translated_name = translate_action_name(other_name, &other_action, cx);
            let msg = match conflict.kind {
                ConflictKind::ChordPrefix => {
                    let chord = conflict.chord_keystroke.as_deref().unwrap_or(&conflict.keystroke);
                    i18n!(cx, "keybindings.conflict_chord_prefix")
                        .replace("{chord}", chord)
                        .replace("{action}", &translated_name)
                }
                _ => i18n!(cx, "keybindings.conflict_with").replace("{action}", &translated_name),
            };
            self.pending_conflict = Some(msg);
        } else {
            self.pending_conflict = None;
        }

        // Update the config
        update_config(|config| {
            config.update_binding(&editing.action, editing.entry_index, new_keystroke.clone());
        });

        // Global conflict check
        let conflicts = get_config().detect_conflicts();
        if !conflicts.is_empty() && self.pending_conflict.is_none() {
            let conflict_msgs: Vec<String> = conflicts.iter().map(|c| c.to_string()).collect();
            self.pending_conflict = Some(conflict_msgs.join("; "));
        }

        // Reload bindings in GPUI
        cx.emit(KeybindingsHelpEvent::ReloadBindings);

        cx.notify();
    }

    /// Add a new empty binding for an action
    fn add_binding_for_action(&mut self, action: &str, context: Option<String>, cx: &mut Context<Self>) {
        let entry = KeybindingEntry::new("unset", context.as_deref());

        update_config(|config| {
            config.add_binding(action, entry);
        });

        // Start recording for the new entry
        let new_index = get_config()
            .bindings
            .get(action)
            .map(|e| e.len().saturating_sub(1))
            .unwrap_or(0);

        self.start_recording(action.to_string(), new_index, cx);
    }

    /// Toggle enabled/disabled state
    fn toggle_binding_entry(&mut self, action: &str, entry_index: usize, cx: &mut Context<Self>) {
        update_config(|config| {
            config.toggle_binding(action, entry_index);
        });

        self.pending_conflict = None;
        cx.emit(KeybindingsHelpEvent::ReloadBindings);
        cx.notify();
    }

    /// Reset a single action to defaults
    fn reset_single_action(&mut self, action: &str, cx: &mut Context<Self>) {
        update_config(|config| {
            config.reset_single_action(action);
        });

        self.pending_conflict = None;
        cx.emit(KeybindingsHelpEvent::ReloadBindings);
        cx.notify();
    }

    /// Build the list of display rows and dynamically count matching items per category
    fn collect_display_rows(&self, cx: &App) -> (Vec<DisplayRow>, std::collections::HashMap<&'static str, usize>) {
        let config = get_config();
        let defaults = KeybindingConfig::defaults();
        let descriptions = get_action_descriptions();
        let query = self.search_query.trim().to_lowercase();

        let mut rows = Vec::new();
        let mut category_counts = std::collections::HashMap::new();

        // Track seen actions for stable ordering
        let mut seen_actions = std::collections::HashSet::new();

        for (action, entries) in &config.bindings {
            if seen_actions.contains(action) {
                continue;
            }
            seen_actions.insert(action.clone());

            let Some(desc) = descriptions.get(action.as_str()) else {
                continue;
            };
            let raw_category = desc.category;
            let category = normalize_category(raw_category);

            let raw_name = desc.name;
            let raw_desc = desc.description;

            let action_name = translate_action_name(raw_name, action, cx);
            let action_description = translate_action_desc(raw_desc, cx);

            for (entry_idx, entry) in entries.iter().enumerate() {
                let is_customized = is_entry_customized(action, entry_idx, entry, &defaults);

                // Filter by platform
                if !is_entry_matching_platform(entry, is_customized) {
                    continue;
                }

                // Check if this row matches the search query
                let matches_search = if query.is_empty() {
                    true
                } else {
                    let formatted_ks = format_keystroke(&entry.keystroke).to_lowercase();
                    let raw_ks = entry.keystroke.to_lowercase();
                    let matches_name = action_name.to_lowercase().contains(&query) || raw_name.to_lowercase().contains(&query);
                    let matches_desc = action_description.to_lowercase().contains(&query) || raw_desc.to_lowercase().contains(&query);
                    let matches_cat = category.to_lowercase().contains(&query) || category_i18n(category, cx).to_lowercase().contains(&query);
                    let matches_ks = formatted_ks.contains(&query) || raw_ks.contains(&query);
                    matches_name || matches_desc || matches_cat || matches_ks
                };

                // Dynamically update category counts based on search match
                if matches_search {
                    *category_counts.entry(category).or_insert(0) += 1;
                }

                // Filter by selected tab for final rendered row list
                if let Some(tab_cat) = self.selected_tab {
                    if tab_cat != category {
                        continue;
                    }
                }

                if !matches_search {
                    continue;
                }

                rows.push(DisplayRow {
                    action: action.clone(),
                    entry_index: entry_idx,
                    keystroke: entry.keystroke.clone(),
                    context: entry.context.clone(),
                    is_customized,
                    is_enabled: entry.enabled,
                    action_name: action_name.clone(),
                    action_description: action_description.clone(),
                });
            }
        }

        (rows, category_counts)
    }

    /// Select previous row with keyboard
    fn select_prev(&mut self, total_rows: usize) -> bool {
        if total_rows == 0 {
            return false;
        }
        if self.selected_index > 0 {
            self.selected_index -= 1;
            true
        } else {
            false
        }
    }

    /// Select next row with keyboard
    fn select_next(&mut self, total_rows: usize) -> bool {
        if total_rows == 0 {
            return false;
        }
        if self.selected_index + 1 < total_rows {
            self.selected_index += 1;
            true
        } else {
            false
        }
    }

    /// Scroll list to selected item
    fn scroll_to_selected(&mut self) {
        self.scroll_handle.scroll_to_item(self.selected_index);
    }
}

pub enum KeybindingsHelpEvent {
    Close,
    ReloadBindings,
}

impl EventEmitter<KeybindingsHelpEvent> for KeybindingsHelp {}

impl Render for KeybindingsHelp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);

        if self.search_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "keybindings.search_placeholder"))
                    .pass_enter(true)
            });
            let input_clone = input.clone();
            cx.subscribe(&input_clone, |this: &mut Self, _, _: &InputChangedEvent, cx| {
                if let Some(inp) = this.search_input.as_ref() {
                    this.search_query = inp.read(cx).text().to_string();
                    this.selected_index = 0;
                    this.scroll_to_selected();
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

        let (rows, category_counts) = self.collect_display_rows(cx);
        let total_rows = rows.len();

        if self.selected_index >= total_rows && total_rows > 0 {
            self.selected_index = total_rows - 1;
        }

        let focus_handle = self.focus_handle.clone();

        // Precompute tooltips and localized strings for closures
        let tip_add = i18n!(cx, "keybindings.tip_add");
        let tip_reset_single = i18n!(cx, "keybindings.tip_reset_single");
        let tip_record = i18n!(cx, "keybindings.tip_record");
        let tip_enabled = i18n!(cx, "keybindings.tip_enabled");
        let tip_disabled = i18n!(cx, "keybindings.tip_disabled");
        let custom_badge_text = i18n!(cx, "keybindings.custom_badge");
        let press_keys_text = i18n!(cx, "keybindings.press_keys");
        let unset_text = i18n!(cx, "keybindings.unset");
        let no_results_text = i18n!(cx, "keybindings.no_results");

        modal_content("keybindings-modal", cx)
            .w(px(700.0))
            .max_h(px(680.0))
            .track_focus(&focus_handle)
            .key_context("KeybindingsHelp")
            .on_action(cx.listener(|this, _: &ShowKeybindings, _window, cx| {
                this.close(cx);
            }))
            .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                if this.editing.is_some() {
                    this.cancel_recording(cx);
                } else if !this.search_query.is_empty() {
                    if let Some(inp) = this.search_input.as_ref() {
                        inp.update(cx, |i, cx| i.set_text("", cx));
                    }
                    this.search_query.clear();
                    this.selected_index = 0;
                    cx.notify();
                } else {
                    this.close(cx);
                }
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _window, cx| {
                if this.editing.is_some() {
                    if event.keystroke.key == "escape" {
                        this.cancel_recording(cx);
                    }
                    return;
                }

                match event.keystroke.key.as_str() {
                    "up" => {
                        let (rows, _) = this.collect_display_rows(cx);
                        if this.select_prev(rows.len()) {
                            this.scroll_to_selected();
                            cx.notify();
                        }
                    }
                    "down" => {
                        let (rows, _) = this.collect_display_rows(cx);
                        if this.select_next(rows.len()) {
                            this.scroll_to_selected();
                            cx.notify();
                        }
                    }
                    "enter" => {
                        let (rows, _) = this.collect_display_rows(cx);
                        if let Some(row) = rows.get(this.selected_index) {
                            this.start_recording(row.action.clone(), row.entry_index, cx);
                        }
                    }
                    "space" => {
                        // Toggle enabled status if search query is empty
                        if this.search_query.is_empty() {
                            let (rows, _) = this.collect_display_rows(cx);
                            if let Some(row) = rows.get(this.selected_index) {
                                this.toggle_binding_entry(&row.action, row.entry_index, cx);
                            }
                        }
                    }
                    "escape" => {
                        if !this.search_query.is_empty() {
                            if let Some(inp) = this.search_input.as_ref() {
                                inp.update(cx, |i, cx| i.set_text("", cx));
                            }
                            this.search_query.clear();
                            this.selected_index = 0;
                            cx.notify();
                        } else {
                            this.close(cx);
                        }
                    }
                    _ => {}
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    // Modal Header (Clean, without redundant subtitle)
                    .child(
                        modal_header(
                            &i18n!(cx, "keybindings.title"),
                            None::<&str>,
                            &t,
                            cx,
                            cx.listener(|this, _, _window, cx| this.close(cx)),
                        )
                    )
                    // Search Bar (Clean input, no '>' prefix)
                    .child(
                        div()
                            .px(SPACE_LG)
                            .py(SPACE_SM)
                            .border_b_1()
                            .border_color(p.border_subtle)
                            .when_some(self.search_input.as_ref(), |this, inp| {
                                this.child(velowork_ui::Input::new(inp).cleanable(true))
                            }),
                    )
                    // Category Tabs Bar (8 streamlined categories)
                    .child(
                        div()
                            .px(SPACE_LG)
                            .py(SPACE_XS)
                            .border_b_1()
                            .border_color(p.border_subtle)
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(SPACE_XS)
                            .child({
                                let is_active = self.selected_tab.is_none();
                                let total_count: usize = category_counts.values().sum();
                                let tab_label = format!("{} ({})", category_i18n("All", cx), total_count);
                                tab_style(
                                    div().id("tab-all"),
                                    &t,
                                    is_active,
                                    cx,
                                )
                                .child(tab_label)
                                .when(is_active, |d| d.child(tab_active_indicator(rgb(t.border_active))))
                                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                                    if this.editing.is_some() {
                                        this.cancel_recording(cx);
                                    }
                                    this.selected_tab = None;
                                    this.selected_index = 0;
                                    this.scroll_to_selected();
                                    cx.notify();
                                }))
                            })
                            .children(CATEGORIES.iter().filter_map(|&cat| {
                                let count = *category_counts.get(cat).unwrap_or(&0);
                                if count == 0 && self.selected_tab != Some(cat) && !self.search_query.is_empty() {
                                    return None;
                                }
                                if count == 0 && self.search_query.is_empty() {
                                    return None;
                                }
                                let is_active = self.selected_tab == Some(cat);
                                let tab_label = format!("{} ({})", category_i18n(cat, cx), count);
                                Some(
                                    tab_style(
                                        div().id(ElementId::Name(format!("tab-{}", cat).into())),
                                        &t,
                                        is_active,
                                        cx,
                                    )
                                    .child(tab_label)
                                    .when(is_active, |d| d.child(tab_active_indicator(rgb(t.border_active))))
                                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _window, cx| {
                                        if this.editing.is_some() {
                                            this.cancel_recording(cx);
                                        }
                                        this.selected_tab = Some(cat);
                                        this.selected_index = 0;
                                        this.scroll_to_selected();
                                        cx.notify();
                                    }))
                                )
                            })),
                    )
                    // Conflict warning banner
                    .when_some(self.pending_conflict.as_ref(), |d, conflict_msg| {
                        d.child(
                            div()
                                .px(SPACE_LG)
                                .py(SPACE_SM)
                                .bg(surface_bg_t(t.warning, &t))
                                .border_b_1()
                                .border_color(p.border_subtle)
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(SPACE_MD)
                                        .child(
                                            div()
                                                .text_size(ui_text_xl(cx))
                                                .child("⚠️"),
                                        )
                                        .child(
                                            v_flex()
                                                .gap(px(2.0))
                                                .child(
                                                    div()
                                                        .text_size(ui_text_md(cx))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(rgb(t.text_primary))
                                                        .child(i18n!(cx, "keybindings.conflict_warning")),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(ui_text_ms(cx))
                                                        .text_color(rgb(t.text_secondary))
                                                        .child(conflict_msg.clone()),
                                                ),
                                        ),
                                ),
                        )
                    })
                    // Keybindings Scrollable List with Scrollbar
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .child(
                                div()
                                    .id("keybindings-list")
                                    .size_full()
                                    .max_h(px(420.0))
                                    .overflow_y_scroll()
                                    .track_scroll(&self.scroll_handle)
                                    .px(SPACE_LG)
                                    .py(SPACE_SM)
                                    .when(total_rows == 0, |d| {
                                        d.child(empty_state(no_results_text.clone(), &t, cx))
                                    })
                                    .children(rows.into_iter().enumerate().map(|(idx, row)| {
                                        let is_selected = idx == self.selected_index;
                                        let action_name = row.action.clone();
                                        let action_for_toggle = row.action.clone();
                                        let action_for_reset = row.action.clone();
                                        let action_for_add = row.action.clone();
                                        let entry_idx = row.entry_index;
                                        let enabled = row.is_enabled;
                                        let is_customized = row.is_customized;
                                        let context_str = row.context.clone();
                                        let is_unset = row.keystroke == "unset" || row.keystroke.is_empty();
                                        let keystroke_display = if is_unset {
                                            unset_text.clone()
                                        } else {
                                            format_keystroke(&row.keystroke)
                                        };

                                        // Check if this specific entry is recording
                                        let is_recording = self.editing.as_ref().is_some_and(|e| {
                                            e.action == action_name && e.entry_index == entry_idx
                                        });
                                        let is_waiting_chord = is_recording && self.editing.as_ref().is_some_and(|e| e.waiting_for_chord);

                                        let tip_toggle_str = if enabled { tip_enabled.clone() } else { tip_disabled.clone() };
                                        let tip_add_str = tip_add.clone();
                                        let tip_reset_str = tip_reset_single.clone();
                                        let tip_record_str = tip_record.clone();

                                        div()
                                            .id(ElementId::Name(format!("row-{}-{}", action_name, entry_idx).into()))
                                            .px(SPACE_MD)
                                            .py(SPACE_SM)
                                            .my(px(2.0))
                                            .rounded(RADIUS_STD)
                                            .w_full()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap(SPACE_MD)
                                            .when(!is_selected, |d| d.hover(|s| s.bg(surface_bg_t(t.bg_hover, &t))))
                                            .when(is_selected, |d| {
                                                d.bg(surface_bg_t(t.bg_selection, &t))
                                                    .border_1()
                                                    .border_color(p.border_subtle)
                                            })
                                            .on_mouse_down(MouseButton::Left, {
                                                cx.listener(move |this, _, _window, cx| {
                                                    this.selected_index = idx;
                                                    cx.notify();
                                                })
                                            })
                                            // Left: Action Name & Description & Badges (with truncation protection)
                                            .child(
                                                v_flex()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .gap(px(2.0))
                                                    .child(
                                                        h_flex()
                                                            .items_center()
                                                            .gap(SPACE_SM)
                                                            .child(
                                                                div()
                                                                    .text_size(ui_text_md(cx))
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .text_color(rgb(t.text_primary))
                                                                    .child(row.action_name),
                                                            )
                                                            .when(is_customized, |d| {
                                                                d.child(
                                                                    div()
                                                                        .text_size(ui_text_sm(cx))
                                                                        .px(SPACE_XS)
                                                                        .py(px(1.0))
                                                                        .rounded(RADIUS_MD)
                                                                        .bg(rgb(t.border_active))
                                                                        .text_color(rgb(0xffffff))
                                                                        .child(custom_badge_text.clone()),
                                                                )
                                                            })
                                                            .when_some(context_str.as_ref(), |d, ctx| {
                                                                d.child(
                                                                    div()
                                                                        .text_size(ui_text_sm(cx))
                                                                        .px(SPACE_XS)
                                                                        .py(px(1.0))
                                                                        .rounded(RADIUS_MD)
                                                                        .bg(surface_bg_t(t.bg_secondary, &t))
                                                                        .text_color(rgb(t.text_muted))
                                                                        .child(ctx.clone()),
                                                                )
                                                            }),
                                                    )
                                                    .when(!row.action_description.is_empty(), |d| {
                                                        d.child(
                                                            div()
                                                                .text_size(ui_text_ms(cx))
                                                                .text_color(rgb(t.text_muted))
                                                                .truncate()
                                                                .child(row.action_description),
                                                        )
                                                    }),
                                            )
                                            // Right: Keystroke Pill + Controls (Dot + Add + Reset + Remove)
                                            .child(
                                                h_flex()
                                                    .flex_shrink_0()
                                                    .items_center()
                                                    .gap(SPACE_SM)
                                                    // Keystroke Badge
                                                    .child(
                                                        div()
                                                            .id(ElementId::Name(format!("ks-{}-{}", action_name, entry_idx).into()))
                                                            .cursor_pointer()
                                                            .px(SPACE_MD)
                                                            .h(px(26.0))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .rounded(RADIUS_STD)
                                                            .border_1()
                                                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(tip_record_str.clone())).into())
                                                            .when(is_recording, |d| {
                                                                d.bg(rgb(t.border_active))
                                                                    .border_color(rgb(t.border_active))
                                                                    .text_color(rgb(0xffffff))
                                                            })
                                                            .when(!is_recording && is_unset, |d| {
                                                                d.bg(p.surface_card)
                                                                    .border_color(p.border_subtle)
                                                                    .border_dashed()
                                                                    .hover(|s| s.bg(p.surface_hover))
                                                            })
                                                            .when(!is_recording && !is_unset, |d| {
                                                                d.bg(p.surface_card)
                                                                    .border_color(p.border_subtle)
                                                                    .hover(|s| s.bg(p.surface_hover))
                                                                    .when(!enabled, |d| d.opacity(0.4))
                                                            })
                                                            .text_size(ui_text_md(cx))
                                                            .font_family(if is_unset { "sans-serif" } else { "monospace" })
                                                            .text_color(if is_recording {
                                                                p.text_on_accent
                                                            } else if is_unset {
                                                                p.text_muted
                                                            } else {
                                                                p.text_secondary
                                                            })
                                                            .child(if is_recording {
                                                                if is_waiting_chord {
                                                                    let first = self.editing.as_ref()
                                                                        .and_then(|e| e.first_chord.as_ref())
                                                                        .map(|s| format_keystroke(s))
                                                                        .unwrap_or_default();
                                                                    format!("{} ...", first)
                                                                } else {
                                                                    press_keys_text.clone()
                                                                }
                                                            } else {
                                                                keystroke_display
                                                            })
                                                            .on_mouse_down(MouseButton::Left, {
                                                                let action = action_name.clone();
                                                                cx.listener(move |this, _, _window, cx| {
                                                                    cx.stop_propagation();
                                                                    if this.editing.as_ref().is_some_and(|e| e.action == action && e.entry_index == entry_idx) {
                                                                        this.cancel_recording(cx);
                                                                    } else {
                                                                        this.start_recording(action.clone(), entry_idx, cx);
                                                                    }
                                                                })
                                                            }),
                                                    )
                                                    // Toggle Enabled/Disabled Dot
                                                    .child(
                                                        div()
                                                            .id(ElementId::Name(format!("toggle-{}-{}", action_for_toggle, entry_idx).into()))
                                                            .cursor_pointer()
                                                            .w(px(24.0))
                                                            .h(px(24.0))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .rounded(RADIUS_MD)
                                                            .bg(p.surface_card)
                                                            .border_1()
                                                            .border_color(p.border_subtle)
                                                            .hover(|s| s.bg(p.surface_hover).border_color(p.border_active))
                                                            .text_size(ui_text_md(cx))
                                                            .text_color(if enabled { p.text_muted } else { rgb(t.error).into() })
                                                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(tip_toggle_str.clone())).into())
                                                            .child(if enabled { "●" } else { "○" })
                                                            .on_mouse_down(MouseButton::Left, {
                                                                let action = action_for_toggle.clone();
                                                                cx.listener(move |this, _, _window, cx| {
                                                                    cx.stop_propagation();
                                                                    this.toggle_binding_entry(&action, entry_idx, cx);
                                                                })
                                                            }),
                                                    )
                                                    // Add Keybinding Button (+)
                                                    .child(
                                                        div()
                                                            .id(ElementId::Name(format!("add-{}", action_for_add).into()))
                                                            .cursor_pointer()
                                                            .w(px(24.0))
                                                            .h(px(24.0))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .rounded(RADIUS_MD)
                                                            .bg(p.surface_card)
                                                            .border_1()
                                                            .border_color(p.border_subtle)
                                                            .hover(|s| s.bg(p.surface_hover).border_color(p.border_active).text_color(p.border_active))
                                                            .text_size(ui_text_lg(cx))
                                                            .text_color(p.text_muted)
                                                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(tip_add_str.clone())).into())
                                                            .child("+")
                                                            .on_mouse_down(MouseButton::Left, {
                                                                let action = action_for_add.clone();
                                                                let ctx = context_str.clone();
                                                                cx.listener(move |this, _, _window, cx| {
                                                                    cx.stop_propagation();
                                                                    this.add_binding_for_action(&action, ctx.clone(), cx);
                                                                })
                                                            }),
                                                    )
                                                    // Reset Single Action Button (↺, only if customized)
                                                    .when(is_customized, |d| {
                                                        d.child(
                                                            div()
                                                                .id(ElementId::Name(format!("reset-{}", action_for_reset).into()))
                                                                .cursor_pointer()
                                                                .w(px(24.0))
                                                                .h(px(24.0))
                                                                .flex()
                                                                .items_center()
                                                                .justify_center()
                                                                .rounded(RADIUS_MD)
                                                                .bg(p.surface_card)
                                                                .border_1()
                                                                .border_color(p.border_subtle)
                                                                .hover(|s| s.bg(p.surface_hover).border_color(rgb(t.warning)).text_color(rgb(t.warning)))
                                                                .text_size(ui_text_md(cx))
                                                                .text_color(p.text_muted)
                                                                .tooltip(move |_, cx| cx.new(|_| Tooltip::new(tip_reset_str.clone())).into())
                                                                .child("↺")
                                                                .on_mouse_down(MouseButton::Left, {
                                                                    let action = action_for_reset.clone();
                                                                    cx.listener(move |this, _, _window, cx| {
                                                                        cx.stop_propagation();
                                                                        this.reset_single_action(&action, cx);
                                                                    })
                                                                }),
                                                        )
                                                     }),
                                            )
                                    })),
                            )
                            .child(Scrollbar::vertical(&self.scroll_handle)),
                    )
                    // Footer: Keyboard hints on left + Reset to Defaults on right
                    .child(
                        div()
                            .px(SPACE_LG)
                            .py(SPACE_SM)
                            .border_t_1()
                            .border_color(p.border_subtle)
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(SPACE_MD)
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(SPACE_MD)
                                    .child(keyboard_hint("↑↓", i18n!(cx, "keybindings.hint_navigate"), &t, cx))
                                    .child(keyboard_hint("Enter", i18n!(cx, "keybindings.hint_record"), &t, cx))
                                    .child(keyboard_hint("Space", i18n!(cx, "keybindings.hint_toggle"), &t, cx))
                                    .child(keyboard_hint("Esc", i18n!(cx, "common.close"), &t, cx)),
                            )
                            .child(
                                div()
                                    .when(self.show_reset_confirmation, |d| {
                                        d.flex()
                                            .items_center()
                                            .gap(SPACE_SM)
                                            .child(
                                                div()
                                                    .text_size(ui_text_ms(cx))
                                                    .text_color(rgb(t.text_muted))
                                                    .child(i18n!(cx, "keybindings.reset_all_confirm")),
                                            )
                                            .child(
                                                div()
                                                    .id("reset-confirm-btn")
                                                    .cursor_pointer()
                                                    .px(SPACE_SM)
                                                    .py(px(2.0))
                                                    .rounded(RADIUS_STD)
                                                    .bg(rgb(t.error))
                                                    .text_size(ui_text_sm(cx))
                                                    .text_color(rgb(0xffffff))
                                                    .child(i18n!(cx, "keybindings.confirm"))
                                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                                                        this.handle_reset_to_defaults(cx);
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .id("reset-cancel-btn")
                                                    .cursor_pointer()
                                                    .px(SPACE_SM)
                                                    .py(px(2.0))
                                                    .rounded(RADIUS_STD)
                                                    .bg(surface_bg_t(t.bg_secondary, &t))
                                                    .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                                                    .text_size(ui_text_sm(cx))
                                                    .text_color(rgb(t.text_primary))
                                                    .child(i18n!(cx, "common.cancel"))
                                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                                                        this.cancel_reset(cx);
                                                    })),
                                            )
                                    })
                                    .when(!self.show_reset_confirmation, |d| {
                                        d.child(
                                            div()
                                                .id("reset-defaults-btn")
                                                .cursor_pointer()
                                                .px(SPACE_SM)
                                                .py(px(2.0))
                                                .rounded(RADIUS_STD)
                                                .bg(surface_bg_t(t.bg_secondary, &t))
                                                .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                                                .text_size(ui_text_sm(cx))
                                                .text_color(rgb(t.text_secondary))
                                                .child(i18n!(cx, "keybindings.reset_to_defaults"))
                                                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                                                    this.handle_reset_to_defaults(cx);
                                                })),
                                        )
                                    }),
                            ),
                    )
    }
}

velowork_ui::impl_focusable!(KeybindingsHelp);
