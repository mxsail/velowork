//! Terminal configuration resolution module.
//!
//! Provides the sparse override resolver:
//! `SessionTerminalOptions` (sparse overrides) + `TerminalDefaults` (global baseline) -> `ResolvedTerminalConfig` (concrete final config).

use serde::{Deserialize, Serialize};
use velowork_core::types::{BellStyle, CursorShape};
use velowork_state::SessionTerminalOptions;

/// Global baseline terminal defaults extracted from `AppSettings`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerminalDefaults {
    pub font_family: String,
    pub font_size: f32,
    pub color_scheme: String,
    pub cursor_shape: CursorShape,
    pub cursor_blink: bool,
    pub bell_style: BellStyle,
    pub bell_cooldown_ms: u32,
    pub scrollback_lines: u32,
    pub word_separators: String,
    pub charset: String,
    pub term_type: String,
    pub shell_integration: bool,
    pub bracketed_paste: bool,
    pub osc52_clipboard: bool,
    pub true_color: bool,
}

impl Default for TerminalDefaults {
    fn default() -> Self {
        Self {
            font_family: "JetBrains Mono".to_string(),
            font_size: 14.0,
            color_scheme: "default".to_string(),
            cursor_shape: CursorShape::Bar,
            cursor_blink: true,
            bell_style: BellStyle::Visual,
            bell_cooldown_ms: 500,
            scrollback_lines: 10000,
            word_separators: " `/\\()\"':,.;<>~!@#$%^&*|+=[]{}`~?".to_string(),
            charset: "UTF-8".to_string(),
            term_type: velowork_core::DEFAULT_TERM_TYPE.to_string(),
            shell_integration: true,
            bracketed_paste: true,
            osc52_clipboard: false,
            true_color: true,
        }
    }
}

/// Fully resolved, concrete terminal configuration.
///
/// Contains no `Option<T>` for overrides — all fields are guaranteed to hold concrete values
/// ready for consumption by the terminal grid, PTY negotiation, and GPUI rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTerminalConfig {
    pub font_family: String,
    pub font_size: f32,
    pub color_scheme: String,
    pub cursor_shape: CursorShape,
    pub cursor_blink: bool,
    pub bell_style: BellStyle,
    pub bell_cooldown_ms: u32,
    pub scrollback_lines: u32,
    pub word_separators: String,
    pub charset: String,
    pub term_type: String,
    pub shell_integration: bool,
    pub bracketed_paste: bool,
    pub osc52_clipboard: bool,
    pub true_color: bool,
}

/// Pure function resolver: merges session overrides on top of global defaults.
///
/// If a session option is `None`, it cleanly falls back to the corresponding global default.
#[must_use]
pub fn resolve_effective_terminal_config(
    options: &SessionTerminalOptions,
    defaults: &TerminalDefaults,
) -> ResolvedTerminalConfig {
    ResolvedTerminalConfig {
        font_family: options
            .font_family
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| defaults.font_family.clone()),
        font_size: options.font_size.unwrap_or(defaults.font_size),
        color_scheme: options
            .color_scheme
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| defaults.color_scheme.clone()),
        cursor_shape: options.cursor_shape.unwrap_or(defaults.cursor_shape),
        cursor_blink: options.cursor_blink.unwrap_or(defaults.cursor_blink),
        bell_style: options.bell_style.unwrap_or(defaults.bell_style),
        bell_cooldown_ms: options.bell_cooldown_ms.unwrap_or(defaults.bell_cooldown_ms),
        scrollback_lines: options.scrollback_lines.unwrap_or(defaults.scrollback_lines),
        word_separators: options
            .word_separators
            .clone()
            .unwrap_or_else(|| defaults.word_separators.clone()),
        charset: options
            .charset
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| defaults.charset.clone()),
        term_type: options
            .term_type
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| defaults.term_type.clone()),
        shell_integration: options.shell_integration.unwrap_or(defaults.shell_integration),
        bracketed_paste: options.bracketed_paste.unwrap_or(defaults.bracketed_paste),
        osc52_clipboard: options.osc52_clipboard.unwrap_or(defaults.osc52_clipboard),
        true_color: options.true_color.unwrap_or(defaults.true_color),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use velowork_state::SshSession;

    #[test]
    fn test_all_inherit() {
        let defaults = TerminalDefaults::default();
        let options = SessionTerminalOptions::default();
        let resolved = resolve_effective_terminal_config(&options, &defaults);

        assert_eq!(resolved.font_family, defaults.font_family);
        assert_eq!(resolved.font_size, defaults.font_size);
        assert_eq!(resolved.color_scheme, defaults.color_scheme);
        assert_eq!(resolved.cursor_shape, defaults.cursor_shape);
        assert_eq!(resolved.cursor_blink, defaults.cursor_blink);
        assert_eq!(resolved.bell_style, defaults.bell_style);
        assert_eq!(resolved.bell_cooldown_ms, defaults.bell_cooldown_ms);
        assert_eq!(resolved.scrollback_lines, defaults.scrollback_lines);
        assert_eq!(resolved.word_separators, defaults.word_separators);
        assert_eq!(resolved.charset, defaults.charset);
        assert_eq!(resolved.term_type, defaults.term_type);
        assert_eq!(resolved.shell_integration, defaults.shell_integration);
        assert_eq!(resolved.bracketed_paste, defaults.bracketed_paste);
        assert_eq!(resolved.osc52_clipboard, defaults.osc52_clipboard);
        assert_eq!(resolved.true_color, defaults.true_color);
    }

    #[test]
    fn test_partial_override() {
        let defaults = TerminalDefaults::default();
        let options = SessionTerminalOptions {
            font_size: Some(18.0),
            charset: Some("GBK".to_string()),
            cursor_shape: Some(CursorShape::Underline),
            bell_style: Some(BellStyle::Audible),
            bell_cooldown_ms: Some(0), // Explicit 0ms override (no throttle)
            word_separators: Some("".to_string()), // Explicit override to empty
            ..Default::default()
        };
        let resolved = resolve_effective_terminal_config(&options, &defaults);

        assert_eq!(resolved.font_size, 18.0);
        assert_eq!(resolved.charset, "GBK");
        assert_eq!(resolved.cursor_shape, CursorShape::Underline);
        assert_eq!(resolved.bell_style, BellStyle::Audible);
        assert_eq!(resolved.bell_cooldown_ms, 0);
        assert_eq!(resolved.word_separators, "");
        assert_eq!(resolved.cursor_blink, defaults.cursor_blink); // Inherited
        // Non-overridden options inherit cleanly
        assert_eq!(resolved.font_family, defaults.font_family);
        assert_eq!(resolved.color_scheme, defaults.color_scheme);
        assert_eq!(resolved.scrollback_lines, defaults.scrollback_lines);
    }

    #[test]
    fn test_global_mutation_dynamic_reflection() {
        let mut defaults = TerminalDefaults::default();
        let options = SessionTerminalOptions {
            font_size: Some(16.0),
            ..Default::default()
        };

        let resolved_before = resolve_effective_terminal_config(&options, &defaults);
        assert_eq!(resolved_before.font_size, 16.0);
        assert_eq!(resolved_before.color_scheme, "default");

        // Global defaults mutate
        defaults.color_scheme = "Solarized Dark".to_string();
        defaults.font_family = "Fira Code".to_string();

        let resolved_after = resolve_effective_terminal_config(&options, &defaults);
        assert_eq!(resolved_after.font_size, 16.0); // Override kept
        assert_eq!(resolved_after.color_scheme, "Solarized Dark"); // Inherited mutated default
        assert_eq!(resolved_after.font_family, "Fira Code");
    }

    #[test]
    fn test_tri_state_capabilities() {
        let defaults = TerminalDefaults {
            shell_integration: true,
            bracketed_paste: true,
            osc52_clipboard: false,
            true_color: true,
            ..Default::default()
        };

        // Case A: all None (inherit)
        let opts_inherit = SessionTerminalOptions::default();
        let res_a = resolve_effective_terminal_config(&opts_inherit, &defaults);
        assert!(res_a.shell_integration);
        assert!(!res_a.osc52_clipboard);

        // Case B: force override
        let opts_override = SessionTerminalOptions {
            shell_integration: Some(false), // Force off
            osc52_clipboard: Some(true),    // Force on
            ..Default::default()
        };
        let res_b = resolve_effective_terminal_config(&opts_override, &defaults);
        assert!(!res_b.shell_integration);
        assert!(res_b.osc52_clipboard);
    }

    #[test]
    fn test_legacy_telnet_encoding_normalize() {
        // 1. Old telnet encoding present, charset None -> migrated to charset
        let mut session = SshSession {
            telnet_encoding: Some("GBK".to_string()),
            ..Default::default()
        };
        session.normalize();
        assert_eq!(session.terminal.charset, Some("GBK".to_string()));
        assert_eq!(session.telnet_encoding, None);

        // 2. Both present -> charset wins
        let mut session2 = SshSession {
            telnet_encoding: Some("GBK".to_string()),
            terminal: SessionTerminalOptions {
                charset: Some("UTF-8".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        session2.normalize();
        assert_eq!(session2.terminal.charset, Some("UTF-8".to_string()));
        assert_eq!(session2.telnet_encoding, None);

        // 3. Whitespace only telnet encoding -> ignored
        let mut session3 = SshSession {
            telnet_encoding: Some("   ".to_string()),
            ..Default::default()
        };
        session3.normalize();
        assert_eq!(session3.terminal.charset, None);
        assert_eq!(session3.telnet_encoding, None);
    }

    #[test]
    fn test_local_env_incremental_merge() {
        let mut system_env = HashMap::new();
        system_env.insert("PATH".to_string(), "/usr/bin".to_string());
        system_env.insert("LANG".to_string(), "en_US.UTF-8".to_string());
        system_env.insert("USER".to_string(), "testuser".to_string());

        let mut local_env = HashMap::new();
        local_env.insert("LANG".to_string(), "zh_CN.UTF-8".to_string());
        local_env.insert("CUSTOM_VAR".to_string(), "hello".to_string());

        // Incremental merge: process_env + session.local_env
        let mut merged = system_env;
        for (k, v) in local_env {
            merged.insert(k, v);
        }

        assert_eq!(merged.get("PATH").map(String::as_str), Some("/usr/bin"));
        assert_eq!(merged.get("USER").map(String::as_str), Some("testuser"));
        assert_eq!(merged.get("LANG").map(String::as_str), Some("zh_CN.UTF-8")); // Overridden
        assert_eq!(merged.get("CUSTOM_VAR").map(String::as_str), Some("hello")); // Injected
    }
}
