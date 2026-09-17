use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::types::{KeybindingConflict, KeybindingEntry};

/// Complete keybinding configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeybindingConfig {
    /// Version for config migration
    #[serde(default = "default_version")]
    pub version: u32,
    /// Map from action name to list of keybindings
    pub bindings: HashMap<String, Vec<KeybindingEntry>>,
}

fn default_version() -> u32 {
    1
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        Self::defaults()
    }
}

/// 判断单键绑定是否会遮挡和弦绑定：
/// - (None, None) => true (同为全局，单键抢先触发)
/// - (Some(x), Some(y)) if x == y => true (同一 context，单键抢先触发)
/// - (Some(_), None) => true (在局部 context 下按前缀键，单键优先于全局和弦)
/// - (None, Some(_)) => false (全局单键不阻止特定 context 内的和弦)
/// - (Some(x), Some(y)) if x != y => false (不同 context 互不影响)
fn single_blocks_chord(single_ctx: Option<&str>, chord_ctx: Option<&str>) -> bool {
    match (single_ctx, chord_ctx) {
        (None, None) => true,
        (Some(x), Some(y)) => x == y,
        (Some(_), None) => true,
        (None, Some(_)) => false,
    }
}

impl KeybindingConfig {
    /// Create default keybinding configuration
    pub fn defaults() -> Self {
        let mut bindings = HashMap::new();

        // Global keybindings
        bindings.insert(
            "Quit".to_string(),
            vec![
                KeybindingEntry::new("cmd-q", None),
                KeybindingEntry::new("ctrl-q", None),
            ],
        );
        bindings.insert(
            "ToggleLeftDock".to_string(),
            vec![
                KeybindingEntry::new("cmd-b", None),
                KeybindingEntry::new("ctrl-b", None),
            ],
        );
        bindings.insert(
            "ToggleRightDock".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-r", None),
                KeybindingEntry::new("ctrl-shift-r", None),
            ],
        );
        bindings.insert(
            "ToggleLeftDockAutoHide".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-b", None),
                KeybindingEntry::new("ctrl-shift-b", None),
            ],
        );
        bindings.insert(
            "ShowKeybindings".to_string(),
            vec![
                KeybindingEntry::new("cmd-k cmd-s", None),
                KeybindingEntry::new("ctrl-k ctrl-s", None),
            ],
        );
        bindings.insert(
            "ShowThemeSelector".to_string(),
            vec![
                KeybindingEntry::new("cmd-k cmd-t", None),
                KeybindingEntry::new("ctrl-k ctrl-t", None),
            ],
        );
        bindings.insert(
            "ShowCommandPalette".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-p", None),
                KeybindingEntry::new("ctrl-shift-p", None),
            ],
        );
        // Commands panel toggle: shows or hides the bottom script/command writer panel.
        bindings.insert(
            "ToggleCommandsPanel".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-y", None),
                KeybindingEntry::new("ctrl-shift-y", None),
            ],
        );
        // SFTP panel toggle
        bindings.insert(
            "ToggleSftpPanel".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-f", None),
                KeybindingEntry::new("ctrl-shift-f", None),
            ],
        );
        bindings.insert(
            "ShowSettings".to_string(),
            vec![
                KeybindingEntry::new("cmd-,", None),
                KeybindingEntry::new("ctrl-,", None),
            ],
        );
        bindings.insert(
            "OpenSettingsFile".to_string(),
            vec![
                KeybindingEntry::new("cmd-alt-,", None),
                KeybindingEntry::new("ctrl-alt-,", None),
            ],
        );
        bindings.insert(
            "ShowProjectManageDialog".to_string(),
            vec![
                KeybindingEntry::new("cmd-e", None),
                KeybindingEntry::new("ctrl-e", None),
            ],
        );
        bindings.insert(
            "ShowQuickCommandsPanel".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-k", None),
                KeybindingEntry::new("ctrl-shift-k", None),
            ],
        );
        bindings.insert(
            "ShowAiAssistant".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-a", None),
                KeybindingEntry::new("ctrl-shift-a", None),
            ],
        );
        bindings.insert(
            "TerminalInlineAi".to_string(),
            vec![
                KeybindingEntry::new("cmd-i", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-i", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "ShowHistoryPanel".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-h", None),
                KeybindingEntry::new("ctrl-shift-h", None),
            ],
        );
        bindings.insert(
            "ShowTunnelsPanel".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-u", None),
                KeybindingEntry::new("ctrl-shift-u", None),
            ],
        );
        bindings.insert(
            "ShowServicesPanel".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-s", None),
                KeybindingEntry::new("ctrl-shift-s", None),
            ],
        );
        bindings.insert(
            "ShowHelp".to_string(),
            vec![
                KeybindingEntry::new("f1", None),
            ],
        );
        bindings.insert(
            "ShowImportSessionDialog".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-i", None),
                KeybindingEntry::new("ctrl-shift-i", None),
            ],
        );
        bindings.insert(
            "NewSession".to_string(),
            vec![
                KeybindingEntry::new("cmd-n", None),
                KeybindingEntry::new("ctrl-n", None),
            ],
        );

        // Fullscreen keybindings
        bindings.insert(
            "ToggleFullscreen".to_string(),
            vec![
                KeybindingEntry::new("shift-escape", Some("TerminalPane")),
            ],
        );

        // Terminal pane keybindings
        bindings.insert(
            "SplitVertical".to_string(),
            vec![
                KeybindingEntry::new("cmd-d", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-shift-d", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "SplitHorizontal".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-d", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-d", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "AddTab".to_string(),
            vec![
                KeybindingEntry::new("cmd-t", None),
                KeybindingEntry::new("ctrl-shift-t", None),
            ],
        );
        bindings.insert(
            "DuplicateSession".to_string(),
            vec![
                KeybindingEntry::new("cmd-alt-s", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-alt-s", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "DuplicateChannel".to_string(),
            vec![
                KeybindingEntry::new("cmd-alt-c", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-alt-c", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "ReconnectTerminal".to_string(),
            vec![
                KeybindingEntry::new("cmd-alt-r", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-alt-r", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "CloseTerminal".to_string(),
            vec![
                KeybindingEntry::new("cmd-w", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-shift-w", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "MinimizeTerminal".to_string(),
            vec![
                KeybindingEntry::new("cmd-m", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-shift-m", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "Copy".to_string(),
            vec![
                KeybindingEntry::new("cmd-c", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-shift-c", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "Paste".to_string(),
            vec![
                KeybindingEntry::new("cmd-v", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-shift-v", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "ScrollUp".to_string(),
            vec![KeybindingEntry::new("shift-pageup", Some("TerminalPane"))],
        );
        bindings.insert(
            "ScrollDown".to_string(),
            vec![KeybindingEntry::new("shift-pagedown", Some("TerminalPane"))],
        );
        bindings.insert(
            "Search".to_string(),
            vec![
                KeybindingEntry::new("cmd-f", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-f", Some("TerminalPane")),
            ],
        );

        // Zoom keybindings
        bindings.insert(
            "ZoomIn".to_string(),
            vec![
                KeybindingEntry::new("cmd-=", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-=", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "ZoomOut".to_string(),
            vec![
                KeybindingEntry::new("cmd--", Some("TerminalPane")),
                KeybindingEntry::new("ctrl--", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "ResetZoom".to_string(),
            vec![
                KeybindingEntry::new("cmd-0", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-0", Some("TerminalPane")),
            ],
        );

        // Navigation keybindings
        bindings.insert(
            "FocusLeft".to_string(),
            vec![
                KeybindingEntry::new("cmd-alt-left", None),
                KeybindingEntry::new("ctrl-alt-left", None),
            ],
        );
        bindings.insert(
            "FocusRight".to_string(),
            vec![
                KeybindingEntry::new("cmd-alt-right", None),
                KeybindingEntry::new("ctrl-alt-right", None),
            ],
        );
        bindings.insert(
            "FocusUp".to_string(),
            vec![
                KeybindingEntry::new("cmd-alt-up", None),
                KeybindingEntry::new("ctrl-alt-up", None),
            ],
        );
        bindings.insert(
            "FocusDown".to_string(),
            vec![
                KeybindingEntry::new("cmd-alt-down", None),
                KeybindingEntry::new("ctrl-alt-down", None),
            ],
        );
        bindings.insert(
            "FocusNextTerminal".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-]", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-tab", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "FocusPrevTerminal".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-[", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-shift-tab", Some("TerminalPane")),
            ],
        );

        bindings.insert(
            "JumpToPreviousPrompt".to_string(),
            vec![
                KeybindingEntry::new("cmd-up", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-up", Some("TerminalPane")),
            ],
        );
        bindings.insert(
            "JumpToNextPrompt".to_string(),
            vec![
                KeybindingEntry::new("cmd-down", Some("TerminalPane")),
                KeybindingEntry::new("ctrl-down", Some("TerminalPane")),
            ],
        );

        bindings.insert(
            "TogglePaneSwitcher".to_string(),
            vec![
                KeybindingEntry::new("cmd-`", None),
                KeybindingEntry::new("ctrl-`", None),
            ],
        );

        bindings.insert(
            "EqualizeLayout".to_string(),
            vec![
                KeybindingEntry::new("cmd-alt-e", None),
                KeybindingEntry::new("ctrl-alt-e", None),
            ],
        );

        bindings.insert(
            "NewWindow".to_string(),
            vec![
                KeybindingEntry::new("cmd-shift-n", None),
                KeybindingEntry::new("ctrl-shift-n", None),
            ],
        );
        // 面板内语义 Action：f2 触发重命名，仅在注册了 on_action(RenameActiveNode) 的
        // 具体面板 Entity 上生效（如会话/隧道/服务/快捷指令/项目管理面板）。
        // SFTP 面板因 crate 依赖边界保留原始 on_key_down("f2")。
        bindings.insert(
            "RenameActiveNode".to_string(),
            vec![KeybindingEntry::new("f2", None)],
        );
        bindings.insert(
            "CyclePanelNext".to_string(),
            vec![KeybindingEntry::new("f6", None)],
        );
        bindings.insert(
            "CyclePanelPrev".to_string(),
            vec![KeybindingEntry::new("shift-f6", None)],
        );
        bindings.insert(
            "LockApp".to_string(),
            vec![
                KeybindingEntry::new("cmd-alt-l", None),
                KeybindingEntry::new("ctrl-alt-l", None),
            ],
        );
        bindings.insert(
            "FocusLeftDock".to_string(),
            vec![
                KeybindingEntry::new("cmd-1", None),
                KeybindingEntry::new("alt-1", None),
            ],
        );
        bindings.insert(
            "FocusCenterDock".to_string(),
            vec![
                KeybindingEntry::new("cmd-2", None),
                KeybindingEntry::new("alt-2", None),
            ],
        );
        bindings.insert(
            "FocusBottomDock".to_string(),
            vec![
                KeybindingEntry::new("cmd-3", None),
                KeybindingEntry::new("alt-3", None),
            ],
        );
        bindings.insert(
            "FocusRightDock".to_string(),
            vec![
                KeybindingEntry::new("cmd-4", None),
                KeybindingEntry::new("alt-4", None),
            ],
        );

        Self {
            version: 1,
            bindings,
        }
    }

    /// Detect conflicts in the current keybinding configuration
    ///
    /// 分组维度为 `(keystroke, context)`：
    /// - 同一 context 下同一 keystroke 绑定到**不同 action** → `Hard` 冲突（错误）。
    /// - 同一 keystroke 出现在**不同 context**（如终端 `TerminalPane` 与全局 `None`）→
    ///   属于 GPUI 的合法 Context Override，因 `seen` 以 `(keystroke, context)` 为键，
    ///   不会落入同一组，故不报错。
    /// - 同一 keystroke 绑定到**相同 action** → 视为重复注册，不报告。
    /// - 单键绑定作为和弦绑定的前缀（如 ctrl-k 与 ctrl-k ctrl-s）→ `ChordPrefix` 冲突。
    pub fn detect_conflicts(&self) -> Vec<KeybindingConflict> {
        use crate::keybindings::types::ConflictKind;
        let mut conflicts = Vec::new();
        let mut seen: HashMap<(String, Option<String>), String> = HashMap::new();

        // 1. Exact match / Hard conflicts
        for (action, entries) in &self.bindings {
            for entry in entries {
                if !entry.enabled {
                    continue;
                }

                let key = (entry.keystroke.clone(), entry.context.clone());

                if let Some(existing_action) = seen.get(&key) {
                    if existing_action != action {
                        conflicts.push(KeybindingConflict {
                            keystroke: entry.keystroke.clone(),
                            context: entry.context.clone(),
                            action1: existing_action.clone(),
                            action2: action.clone(),
                            kind: ConflictKind::Hard,
                            chord_keystroke: None,
                        });
                    }
                } else {
                    seen.insert(key, action.clone());
                }
            }
        }

        // 2. Chord prefix conflicts
        let mut singles = Vec::new();
        let mut chords = Vec::new();

        for (action, entries) in &self.bindings {
            for entry in entries {
                if !entry.enabled {
                    continue;
                }
                if entry.keystroke.contains(' ') {
                    chords.push((action.clone(), entry.keystroke.clone(), entry.context.clone()));
                } else {
                    singles.push((action.clone(), entry.keystroke.clone(), entry.context.clone()));
                }
            }
        }

        for (s_action, s_key, s_ctx) in &singles {
            for (c_action, c_key, c_ctx) in &chords {
                if s_action == c_action {
                    continue;
                }
                let Some(prefix) = c_key.split_whitespace().next() else {
                    continue;
                };
                if s_key == prefix && single_blocks_chord(s_ctx.as_deref(), c_ctx.as_deref()) {
                    conflicts.push(KeybindingConflict {
                        keystroke: s_key.clone(),
                        context: s_ctx.clone(),
                        action1: s_action.clone(),
                        action2: c_action.clone(),
                        kind: ConflictKind::ChordPrefix,
                        chord_keystroke: Some(c_key.clone()),
                    });
                }
            }
        }

        conflicts
    }

    /// Check if setting a keystroke for an action in a context will conflict with another enabled action
    pub fn check_conflict(
        &self,
        action: &str,
        keystroke: &str,
        context: Option<&str>,
    ) -> Option<KeybindingConflict> {
        use crate::keybindings::types::ConflictKind;
        // 1. Exact match check
        for (act, entries) in &self.bindings {
            if act == action {
                continue;
            }
            for entry in entries {
                if !entry.enabled {
                    continue;
                }
                if entry.keystroke == keystroke && entry.context.as_deref() == context {
                    return Some(KeybindingConflict {
                        keystroke: keystroke.to_string(),
                        context: context.map(|s| s.to_string()),
                        action1: act.clone(),
                        action2: action.to_string(),
                        kind: ConflictKind::Hard,
                        chord_keystroke: None,
                    });
                }
            }
        }

        // 2. Chord prefix check (bidirectional)
        let is_chord = keystroke.contains(' ');
        if is_chord {
            // Direction A: New key is a chord: check if any existing single key blocks its prefix
            let Some(prefix) = keystroke.split_whitespace().next() else {
                return None;
            };
            for (act, entries) in &self.bindings {
                if act == action {
                    continue;
                }
                for entry in entries {
                    if !entry.enabled || entry.keystroke.contains(' ') {
                        continue;
                    }
                    if entry.keystroke == prefix && single_blocks_chord(entry.context.as_deref(), context) {
                        return Some(KeybindingConflict {
                            keystroke: entry.keystroke.clone(),
                            context: entry.context.clone(),
                            action1: act.clone(),
                            action2: action.to_string(),
                            kind: ConflictKind::ChordPrefix,
                            chord_keystroke: Some(keystroke.to_string()),
                        });
                    }
                }
            }
        } else {
            // Direction B: New key is a single key: check if it blocks any existing chord
            for (act, entries) in &self.bindings {
                if act == action {
                    continue;
                }
                for entry in entries {
                    if !entry.enabled || !entry.keystroke.contains(' ') {
                        continue;
                    }
                    let Some(prefix) = entry.keystroke.split_whitespace().next() else {
                        continue;
                    };
                    if prefix == keystroke && single_blocks_chord(context, entry.context.as_deref()) {
                        return Some(KeybindingConflict {
                            keystroke: keystroke.to_string(),
                            context: context.map(|s| s.to_string()),
                            action1: act.clone(),
                            action2: action.to_string(),
                            kind: ConflictKind::ChordPrefix,
                            chord_keystroke: Some(entry.keystroke.clone()),
                        });
                    }
                }
            }
        }

        None
    }

    /// Update the keystroke for a specific binding entry
    pub fn update_binding(&mut self, action: &str, entry_index: usize, new_keystroke: String) {
        if let Some(entries) = self.bindings.get_mut(action)
            && let Some(entry) = entries.get_mut(entry_index) {
                entry.keystroke = new_keystroke;
            }
    }

    /// Reset a single action's bindings back to defaults
    pub fn reset_single_action(&mut self, action: &str) {
        let defaults = Self::defaults();
        if let Some(default_entries) = defaults.bindings.get(action) {
            self.bindings.insert(action.to_string(), default_entries.clone());
        } else {
            // Action doesn't exist in defaults — remove it
            self.bindings.remove(action);
        }
    }

    /// Add a new binding entry for an action
    pub fn add_binding(&mut self, action: &str, entry: KeybindingEntry) {
        self.bindings
            .entry(action.to_string())
            .or_default()
            .push(entry);
    }

    /// Remove a specific binding entry by index
    /// Returns true if the entry was removed
    pub fn remove_binding(&mut self, action: &str, entry_index: usize) -> bool {
        if let Some(entries) = self.bindings.get_mut(action)
            && entry_index < entries.len() {
                entries.remove(entry_index);
                return true;
            }
        false
    }

    /// Toggle the enabled state of a specific binding entry
    pub fn toggle_binding(&mut self, action: &str, entry_index: usize) {
        if let Some(entries) = self.bindings.get_mut(action)
            && let Some(entry) = entries.get_mut(entry_index) {
                entry.enabled = !entry.enabled;
            }
    }

    /// Get all actions that have custom (non-default) bindings
    pub fn get_customized_actions(&self) -> HashSet<String> {
        let defaults = Self::defaults();
        let mut customized = HashSet::new();

        for (action, entries) in &self.bindings {
            if let Some(default_entries) = defaults.bindings.get(action) {
                if entries != default_entries {
                    customized.insert(action.clone());
                }
            } else {
                // Action exists in config but not in defaults
                customized.insert(action.clone());
            }
        }

        // Also check for actions in defaults that are missing from config
        for action in defaults.bindings.keys() {
            if !self.bindings.contains_key(action) {
                customized.insert(action.clone());
            }
        }

        customized
    }

}

/// Get the keybindings configuration file path
pub fn get_keybindings_path() -> PathBuf {
    if let Some(p) = velowork_core::profiles::try_current() {
        p.keybindings_json()
    } else {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("velowork")
            .join("keybindings.json")
    }
}

/// Load keybinding configuration from disk
pub fn load_keybindings() -> KeybindingConfig {
    let path = get_keybindings_path();
    if path.exists()
        && let Ok(content) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<KeybindingConfig>(&content) {
                Ok(mut config) => {
                    let defaults = KeybindingConfig::defaults();

                    // Prune any legacy or obsolete actions that no longer exist in defaults
                    let initial_len = config.bindings.len();
                    config.bindings.retain(|action, _| defaults.bindings.contains_key(action));
                    let had_pruned = config.bindings.len() != initial_len;

                    // Merge in any new default actions missing from the saved config
                    let mut had_added = false;
                    for (action, entries) in &defaults.bindings {
                        if !config.bindings.contains_key(action) {
                            config.bindings.insert(action.clone(), entries.clone());
                            had_added = true;
                        }
                    }

                    // Save cleaned config if obsolete actions were removed or new actions added
                    if had_pruned || had_added {
                        let _ = save_keybindings(&config);
                    }

                    // Check for conflicts and log warnings
                    let conflicts = config.detect_conflicts();
                    for conflict in &conflicts {
                        log::warn!("Keybinding conflict: {}", conflict);
                    }
                    return config;
                }
                Err(e) => {
                    log::warn!("Failed to parse keybindings config: {}, using defaults", e);
                }
            }
        }
    KeybindingConfig::defaults()
}

/// Save keybinding configuration to disk
pub fn save_keybindings(config: &KeybindingConfig) -> anyhow::Result<()> {
    let path = get_keybindings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_no_conflicts() {
        let config = KeybindingConfig::defaults();
        let conflicts = config.detect_conflicts();
        assert!(conflicts.is_empty(), "Default config should have no conflicts: {:?}", conflicts);
    }

    #[test]
    fn test_duplicate_keybinding_detected() {
        let mut config = KeybindingConfig::defaults();
        // Add a binding that conflicts with an existing one
        config.bindings.insert(
            "CustomAction".to_string(),
            vec![KeybindingEntry::new("cmd-b", None)], // conflicts with ToggleLeftDock
        );
        let conflicts = config.detect_conflicts();
        assert!(!conflicts.is_empty(), "Should detect conflict on cmd-b");
        assert!(conflicts.iter().any(|c| c.keystroke == "cmd-b"));
    }

    #[test]
    fn test_same_key_different_context_no_conflict() {
        let mut bindings = HashMap::new();
        bindings.insert(
            "Action1".to_string(),
            vec![KeybindingEntry::new("cmd-d", None)], // global
        );
        bindings.insert(
            "Action2".to_string(),
            vec![KeybindingEntry::new("cmd-d", Some("TerminalPane"))], // scoped
        );
        let config = KeybindingConfig { version: 1, bindings };
        let conflicts = config.detect_conflicts();
        assert!(conflicts.is_empty(), "Different contexts should not conflict");
    }

    /// 终端快捷键对工作台快捷键的合法 Context Override：
    /// `ctrl-shift-c` 在 TerminalPane(Copy) 与全局(NonTerminalAction) 共存不误报。
    #[test]
    fn test_terminal_context_override_no_false_positive() {
        let mut bindings = HashMap::new();
        bindings.insert(
            "TerminalCopy".to_string(),
            vec![KeybindingEntry::new("ctrl-shift-c", Some("TerminalPane"))],
        );
        bindings.insert(
            "NonTerminalAction".to_string(),
            vec![KeybindingEntry::new("ctrl-shift-c", None)],
        );
        let config = KeybindingConfig { version: 1, bindings };
        let conflicts = config.detect_conflicts();
        assert!(
            conflicts.is_empty(),
            "TerminalPane vs Global override should NOT be a conflict: {:?}",
            conflicts
        );
    }

    /// 同一 keystroke 跨三个 context（TerminalPane / Editor / None）共存均为合法 Override。
    #[test]
    fn test_ctrl_shift_t_three_context_override() {
        let mut bindings = HashMap::new();
        bindings.insert(
            "AddTab".to_string(),
            vec![KeybindingEntry::new("ctrl-shift-t", Some("TerminalPane"))],
        );
        bindings.insert(
            "ReopenClosedTab".to_string(),
            vec![KeybindingEntry::new("ctrl-shift-t", Some("Editor"))],
        );
        bindings.insert(
            "ToggleCommandsPanel".to_string(),
            vec![KeybindingEntry::new("ctrl-shift-t", None)],
        );
        let config = KeybindingConfig { version: 1, bindings };
        let conflicts = config.detect_conflicts();
        assert!(
            conflicts.is_empty(),
            "Three-context override of ctrl-shift-t should NOT conflict: {:?}",
            conflicts
        );
    }

    /// 同一 context 同一 keystroke 绑定到不同 Action 必须判定为 Hard Conflict。
    #[test]
    fn test_hard_conflict_same_context_different_action() {
        let mut bindings = HashMap::new();
        bindings.insert(
            "ActionA".to_string(),
            vec![KeybindingEntry::new("ctrl-shift-z", None)],
        );
        bindings.insert(
            "ActionB".to_string(),
            vec![KeybindingEntry::new("ctrl-shift-z", None)],
        );
        let config = KeybindingConfig { version: 1, bindings };
        let conflicts = config.detect_conflicts();
        assert_eq!(conflicts.len(), 1, "Should report exactly one Hard conflict");
        assert_eq!(
            conflicts[0].kind,
            crate::keybindings::types::ConflictKind::Hard
        );
    }

    /// 相同 keystroke 绑定到相同 Action（重复注册）不报告冲突。
    #[test]
    fn test_repeat_same_action_no_conflict() {
        let mut bindings = HashMap::new();
        bindings.insert(
            "ToggleLeftDock".to_string(),
            vec![KeybindingEntry::new("ctrl-b", None)],
        );
        bindings.insert(
            "ToggleLeftDock".to_string(),
            vec![KeybindingEntry::new("ctrl-b", Some("Sidebar"))],
        );
        let config = KeybindingConfig { version: 1, bindings };
        let conflicts = config.detect_conflicts();
        assert!(
            conflicts.is_empty(),
            "Repeated same-action binding should NOT conflict: {:?}",
            conflicts
        );
    }


    #[test]
    fn test_disabled_binding_no_conflict() {
        let mut bindings = HashMap::new();
        bindings.insert(
            "Action1".to_string(),
            vec![KeybindingEntry::new("cmd-x", None)],
        );
        let mut disabled = KeybindingEntry::new("cmd-x", None);
        disabled.enabled = false;
        bindings.insert(
            "Action2".to_string(),
            vec![disabled],
        );
        let config = KeybindingConfig { version: 1, bindings };
        let conflicts = config.detect_conflicts();
        assert!(conflicts.is_empty(), "Disabled binding should not conflict");
    }

    #[test]
    fn test_customized_actions_detected() {
        let mut config = KeybindingConfig::defaults();
        // Modify an existing action's binding
        config.bindings.insert(
            "ToggleLeftDock".to_string(),
            vec![KeybindingEntry::new("ctrl-alt-b", None)], // changed from default
        );
        let customized = config.get_customized_actions();
        assert!(customized.contains("ToggleLeftDock"), "Modified action should be detected");
    }

    #[test]
    fn test_serialization_round_trip() {
        let config = KeybindingConfig::defaults();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: KeybindingConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.version, config.version);
        assert_eq!(deserialized.bindings.len(), config.bindings.len());
    }

    #[test]
    fn test_check_conflict() {
        let config = KeybindingConfig::defaults();
        // Quit uses "ctrl-q" (global) on Windows/Linux and "cmd-q" on macOS
        let conflict = config.check_conflict("SomeNewAction", "ctrl-q", None);
        assert!(conflict.is_some(), "Binding ctrl-q should conflict with Quit");
        assert_eq!(conflict.unwrap().action1, "Quit");

        // Same action should not conflict with itself
        let self_conflict = config.check_conflict("Quit", "ctrl-q", None);
        assert!(self_conflict.is_none(), "Same action should not report conflict with itself");

        // Unused shortcut should not conflict
        let no_conflict = config.check_conflict("SomeNewAction", "ctrl-alt-shift-super-f12", None);
        assert!(no_conflict.is_none(), "Unused keybinding should not conflict");
    }

    #[test]
    fn test_chord_prefix_conflict() {
        use crate::keybindings::types::ConflictKind;
        let config = KeybindingConfig::defaults();

        // 1. Defaults should have zero conflicts (neither Hard nor ChordPrefix)
        let default_conflicts = config.detect_conflicts();
        assert!(
            default_conflicts.is_empty(),
            "Default keybindings should have no conflicts, but found: {:?}",
            default_conflicts
        );

        // 2. check_conflict Direction A: setting a chord whose prefix is already a single key
        // "ctrl-q" is Quit (global). Setting "ctrl-q ctrl-x" should detect ChordPrefix conflict.
        let chord_conflict = config.check_conflict("SomeAction", "ctrl-q ctrl-x", None);
        assert!(chord_conflict.is_some());
        let c = chord_conflict.unwrap();
        assert_eq!(c.kind, ConflictKind::ChordPrefix);
        assert_eq!(c.action1, "Quit");
        assert_eq!(c.chord_keystroke.as_deref(), Some("ctrl-q ctrl-x"));

        // 3. check_conflict Direction B: setting a single key that blocks an existing chord
        // "ctrl-k ctrl-s" is ShowKeybindings. Setting "ctrl-k" should detect ChordPrefix conflict.
        let single_conflict = config.check_conflict("SomeAction", "ctrl-k", None);
        assert!(single_conflict.is_some());
        let c = single_conflict.unwrap();
        assert_eq!(c.kind, ConflictKind::ChordPrefix);
        assert_eq!(c.action1, "ShowKeybindings");
        assert_eq!(c.chord_keystroke.as_deref(), Some("ctrl-k ctrl-s"));
    }
}
