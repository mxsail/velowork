//! Unified search-field component.
//!
//! Combines optional search icon + [`SimpleInput`](crate::simple_input::SimpleInput)
//! with clear button into a single reusable element with sensible defaults.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Minimal — full defaults (icon + standard size + container_height centering)
//! SearchField::new(&self.filter_input, &t, cx)
//!
//! // Customized — hide icon, use compact size
//! SearchField::new(&self.filter_input, &t, cx)
//!     .show_icon(false)
//!     .compact()
//! ```

use crate::input::InputState;
use gpui::prelude::*;
use gpui::*;

/// A pre-styled search input field.
pub struct SearchField<'a> {
    input: &'a Entity<InputState>,
    show_icon: bool,
    compact: bool,
    allow_clear: bool,
}

impl<'a> SearchField<'a> {
    /// Create a new `SearchField` with all defaults enabled.
    pub fn new(
        input: &'a Entity<InputState>,
        _t: &'a crate::theme::ThemeColors,
        _cx: &'a App,
    ) -> Self {
        Self {
            input,
            show_icon: true,
            compact: false,
            allow_clear: true,
        }
    }

    /// Show or hide the leading search icon. Default: `true`.
    pub fn show_icon(mut self, show: bool) -> Self {
        self.show_icon = show;
        self
    }

    /// Use `ControlSize::Compact` instead of `ControlSize::Default`.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Enable or disable the clear button when text is present. Default: `true`.
    pub fn allow_clear(mut self, allow: bool) -> Self {
        self.allow_clear = allow;
        self
    }

    /// Alias for [`allow_clear`].
    #[allow(non_snake_case)]
    pub fn allowClear(self, allow: bool) -> Self {
        self.allow_clear(allow)
    }
}

impl IntoElement for SearchField<'_> {
    type Element = AnyElement;

    fn into_element(self) -> Self::Element {
        let mut input = crate::input::Input::new(self.input).search(self.show_icon);
        if self.compact {
            input = input.compact();
        }
        input.cleanable(self.allow_clear).into_any_element()
    }
}
