//! Reusable UI components.
//!
//! All generic components live in `velowork-ui`. This module re-exports them
//! for backward compatibility with `crate::views::components::*` imports.

// Re-export from velowork-ui
pub use velowork_ui::dropdown::{
    dropdown_anchored_below, dropdown_button, dropdown_button_sized, dropdown_option,
    dropdown_overlay,
};
pub use velowork_ui::overlay::{modal_backdrop, modal_content, modal_header};
pub use velowork_ui::badge::{badge, keyboard_hints_footer};
pub use velowork_ui::button::button;
pub use velowork_ui::input::{labeled_input, search_input_area, search_input_area_selected};
pub use velowork_ui::menu::menu_item;
pub use velowork_ui::simple_input::{InputChangedEvent, SimpleInput, SimpleInputState};

// Re-export as modules for path-based imports (e.g. `crate::views::components::simple_input::SimpleInputState`)
pub use velowork_ui::simple_input;
pub use velowork_ui::quick_picker;

// Re-export from velowork-ui
pub use velowork_ui::quick_picker::{
    handle_quick_picker_key, substring_filter, FilterResult, QuickPickerAction, QuickPickerConfig,
    QuickPickerState,
};

pub use velowork_ui::path_autocomplete::*;
