#![recursion_limit = "2048"]
#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

//! Velowork UI component library.
//!
//! Reusable UI components, design tokens, and theme helpers for the Velowork terminal.

// ---------------------------------------------------------------------------
// Frozen Design Infrastructure (consume only, no new abstractions)
// ---------------------------------------------------------------------------
pub mod design;

// ---------------------------------------------------------------------------
// IDE Core Infrastructure (layout tree, focus graph)
// ---------------------------------------------------------------------------
pub mod ide;

// ---------------------------------------------------------------------------
// Composable Behavior Modifiers (hover, focus ring, active feedback)
// ---------------------------------------------------------------------------
pub mod behavior;

pub mod badge;
pub mod brand_logo;
pub mod button;
pub mod capsule_toolbar;
pub use capsule_toolbar::*;
pub mod checkbox;
pub mod chip;
pub mod click_detector;
pub mod code_block;
pub mod color_dot;
pub mod color_picker;
pub mod color_utils;
pub mod confirm_dialog;
pub mod context_menu_backdrop;
pub mod decorations;
pub mod dialog_actions;
pub mod dock;
pub mod dropdown;
pub mod empty_state;
pub mod expand;
pub mod file_icon;
pub mod focus_group;
pub mod focusable;
pub use focusable::FocusSurfaceExt;
pub mod form;
pub mod header_buttons;
pub mod icon;
pub mod icon_action_button;
pub mod icon_button;
pub mod input;
pub mod list_row;
pub mod menu;
pub mod motion;
pub mod overlay;
pub mod overlay_menu;
pub mod overlay_registry;
pub mod path_autocomplete;
pub mod popover;
pub mod quick_picker;
pub mod radio;
pub mod rename_directory_dialog;
pub mod rename_state;
pub mod scrollable;
pub mod search_field;
pub mod selectable_list;
pub mod select;
pub mod settings;
pub mod simple_input;
pub mod slider;
pub mod spinner;
pub mod stepper;
pub mod styled;
pub mod switch;
pub mod syntax;
pub mod tab;
pub mod terminal_preview;
pub mod text_utils;
pub mod theme;
pub mod title_bar;
pub mod title_subtitle;
pub mod tokens;
pub mod tooltip;
pub mod tree;
pub mod typography;
pub mod virtual_list;
pub use behavior::{
    ElementBehaviorExt, HoverBehavior, SelectedBehavior, StatefulElementBehaviorExt,
};
pub use brand_logo::brand_logo;
pub use button::{Button, ButtonIcon, button, button_primary};
pub use checkbox::Checkbox;
pub use design::appearance::{
    ControlAppearance, ControlSize, ControlVariant, control_height, control_height_for_size,
    control_line_height, tree_row_height,
};
pub use design::density::UiDensity;
pub use design::semantic::SemanticPalette;
pub use focus_group::{FocusGroup, FocusGroupExt};
pub use form::{Form, FormItem, FormLayout, form, form_item};
pub use icon_button::{icon_button, FocusableIconButtonExt};
pub use input::{
    Input, InputContentType, InputEvent, InputFocusRingExt, InputMode, InputState,
    Textarea, TextareaState, focus_ring_shadows,
};
pub use switch::Switch;
pub use overlay::{AnimatedModal, AnimatedModalEvent, ModalAlignment, ModalAnimationsEnabled, WindowCornerRadius};
pub use radio::{RadioGroup, RadioMode, RadioOption};
pub use search_field::SearchField;
pub use select::{Select, SelectEvent, SelectGroup, SelectOption, SelectState};
pub use slider::{Slider, SliderEvent, SliderScale, SliderState, SliderValue};
pub use stepper::{NumberStepper, number_stepper};
pub use styled::{h_flex, v_flex};
pub use tab::{TAB_HEIGHT, tab_height, tab_h_padding, tab_active_indicator, tab_style};
pub use terminal_preview::{TerminalPreviewProps, terminal_preview_card};
pub use tooltip::Tooltip;
pub use tree::{
    Tree, TreeNodeContext, TreeNodeData, tree, tree_row_appearance, expandable_folder_row,
    expandable_file_row, FileTreeNode, build_file_tree,
};
pub use virtual_list::{ListSelection, scroll_to_row, virtual_list};

// Re-export decoupled overlay components (moved out of velowork-views-sidebar).
pub use color_picker::{ColorPicker, ColorPickerEvent, PRESET_COLORS};
pub use rename_directory_dialog::{RenameDirectoryDialog, RenameDirectoryDialogEvent};
pub use path_autocomplete::{render_path_suggestions, PathAutoCompleteEvent, PathAutoCompleteState, PathSuggestion};

// Generic cancel action, shared by overlays (context menus, dialogs, popovers).
gpui::actions!(velowork_ui, [Cancel]);
