use gpui::Global;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Embedded translation files ──────────────────────────────────────────────

static EN_JSON: &str = include_str!("../locales/en.json");
static ZH_JSON: &str = include_str!("../locales/zh.json");

// ── Locale type ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    #[default]
    En,
    Zh,
}

impl Locale {
    pub fn display_name(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::Zh => "中文",
        }
    }

    pub fn all_variants() -> &'static [Locale] {
        &[Locale::En, Locale::Zh]
    }

    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Zh => "zh",
        }
    }
}

// ── Translation store ───────────────────────────────────────────────────────

#[derive(Debug)]
struct TranslationStore {
    translations: HashMap<Locale, HashMap<String, String>>,
}

impl TranslationStore {
    fn new() -> Self {
        let mut translations = HashMap::new();
        translations.insert(Locale::En, load_translations(EN_JSON));
        translations.insert(Locale::Zh, load_translations(ZH_JSON));
        Self { translations }
    }

    fn translate(&self, locale: Locale, key: &str) -> String {
        let Some(map) = self.translations.get(&locale) else {
            return key.to_string();
        };

        // 1. Exact match (O(1))
        if let Some(val) = map.get(key) {
            return val.clone();
        }

        // 2. Common prefix fallback (e.g. "cancel" -> "common.action.cancel")
        let common_prefixes = [
            "common.",
            "common.action.",
            "common.state.",
            "common.status.",
            "common.navigation.",
        ];
        for prefix in &common_prefixes {
            let candidate = format!("{}{}", prefix, key);
            if let Some(val) = map.get(&candidate) {
                return val.clone();
            }
        }

        // 3. Fallback for un-prefixed single words to common namespace
        if !key.contains('.') {
            let suffix_dot = format!(".{}", key);
            for (k, v) in map {
                if k.ends_with(&suffix_dot) && k.starts_with("common.") {
                    return v.clone();
                }
            }
        }

        key.to_string()
    }
}

fn load_translations(json: &str) -> HashMap<String, String> {
    let value: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::Value::Null);
    let mut map = HashMap::new();
    flatten_translations(&value, String::new(), &mut map);
    map
}

/// Recursively flatten a nested JSON object into dot-separated keys.
///
/// e.g. `{"a": {"b": "x", "c": {"d": "y"}}}` -> `{"a.b": "x", "a.c.d": "y"}`.
/// A scalar value stored at a path that also has child objects (e.g. a label
/// living alongside its sub-keys) is preserved under the `_label` sub-key.
fn flatten_translations(value: &serde_json::Value, prefix: String, out: &mut HashMap<String, String>) {
    match value {
        serde_json::Value::Object(obj) => {
            for (k, v) in obj {
                let new_prefix = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", prefix, k)
                };
                flatten_translations(v, new_prefix, out);
            }
        }
        serde_json::Value::String(s) => {
            out.insert(prefix, s.clone());
        }
        _ => {}
    }
}

// ── Global locale state ─────────────────────────────────────────────────────

static STORE: std::sync::OnceLock<TranslationStore> = std::sync::OnceLock::new();

fn store() -> &'static TranslationStore {
    STORE.get_or_init(TranslationStore::new)
}

/// GPUI Global holding the current locale.
#[derive(Clone)]
pub struct GlobalLocale(pub Locale);

impl Global for GlobalLocale {}

/// Initialize the global locale. Call once at app startup.
pub fn init_locale(locale: Locale, cx: &mut gpui::App) {
    cx.set_global(GlobalLocale(locale));
}

/// Get the current locale from the GPUI context.
pub fn current_locale(cx: &gpui::App) -> Locale {
    cx.global::<GlobalLocale>().0
}

/// Set the current locale at runtime (triggers UI refresh).
pub fn set_locale(locale: Locale, cx: &mut gpui::App) {
    cx.set_global(GlobalLocale(locale));
    cx.refresh_windows();
}

/// Translate a key using the current locale from the GPUI context.
pub fn t(cx: &gpui::App, key: &str) -> String {
    let locale = current_locale(cx);
    store().translate(locale, key)
}

/// Translate a key with format arguments using the current locale.
pub fn t_fmt(cx: &gpui::App, key: &str, args: &[(&str, &str)]) -> String {
    let locale = current_locale(cx);
    let mut result = store().translate(locale, key);
    for (placeholder, value) in args {
        result = result.replace(&format!("{{{}}}", placeholder), value);
    }
    result
}

/// Translate a key using the current locale from a mutable App context.
pub fn t_cx(cx: &mut gpui::App, key: &str) -> String {
    t(&*cx, key)
}

/// Macro for translating keys in view render methods.
/// Usage: `i18n!(cx, "dock.explorer")`
/// Note: `cx` must implement `Deref<Target = gpui::App>` (e.g., `&mut Context<Self>` or `&App`).
#[macro_export]
macro_rules! i18n {
    ($cx:expr, $key:expr) => {
        $crate::t(&$cx, $key)
    };
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_english() {
        let store = TranslationStore::new();
        assert_eq!(store.translate(Locale::En, "app.name"), "Velowork");
    }

    #[test]
    fn translate_chinese() {
        let store = TranslationStore::new();
        assert_eq!(store.translate(Locale::Zh, "app.name"), "Velowork");
    }

    #[test]
    fn missing_key_returns_key() {
        let store = TranslationStore::new();
        assert_eq!(store.translate(Locale::En, "nonexistent"), "nonexistent");
    }

    #[test]
    fn locale_display_names() {
        assert_eq!(Locale::En.display_name(), "English");
        assert_eq!(Locale::Zh.display_name(), "中文");
    }

    #[test]
    fn nested_json_is_flattened() {
        let store = TranslationStore::new();
        // Nested structure is flattened into dot-separated keys.
        assert_eq!(store.translate(Locale::Zh, "dock.explorer"), "会话管理器");
        assert_eq!(store.translate(Locale::En, "common.action.cancel"), "Cancel");
        assert_eq!(
            store.translate(Locale::Zh, "search_dialogs.file_search.title"),
            "跳转到文件"
        );
    }

    #[test]
    fn test_locales_keys_match() {
        let zh_map = load_translations(ZH_JSON);
        let en_map = load_translations(EN_JSON);

        let mut zh_only: Vec<_> = zh_map
            .keys()
            .filter(|k| !en_map.contains_key(*k))
            .collect();
        let mut en_only: Vec<_> = en_map
            .keys()
            .filter(|k| !zh_map.contains_key(*k))
            .collect();

        zh_only.sort();
        en_only.sort();

        assert!(
            zh_only.is_empty() && en_only.is_empty(),
            "Locale keys mismatch!\nOnly in zh.json ({}): {:?}\nOnly in en.json ({}): {:?}",
            zh_only.len(),
            zh_only,
            en_only.len(),
            en_only
        );
    }

    #[test]
    fn test_no_anonymous_placeholders() {
        let zh_map = load_translations(ZH_JSON);
        let en_map = load_translations(EN_JSON);

        let mut errors = Vec::new();
        for (k, v) in zh_map.iter().chain(en_map.iter()) {
            // Find '{}' that is not part of '{{...}}'
            let mut i = 0;
            let bytes = v.as_bytes();
            while i + 1 < bytes.len() {
                if bytes[i] == b'{' && bytes[i + 1] == b'}' {
                    let prev_brace = i > 0 && bytes[i - 1] == b'{';
                    let next_brace = i + 2 < bytes.len() && bytes[i + 2] == b'}';
                    if !prev_brace && !next_brace {
                        errors.push(format!("Key '{k}' contains anonymous placeholder '{{}}': {v}"));
                    }
                }
                i += 1;
            }
        }
        assert!(errors.is_empty(), "Anonymous placeholders found:\n{}", errors.join("\n"));
    }

    #[test]
    #[allow(clippy::collapsible_if)]
    fn test_no_duplicate_json_keys() {
        fn check_json(json: &str, file_name: &str) {
            let mut duplicates = Vec::new();
            let mut stack: Vec<std::collections::HashSet<String>> = vec![std::collections::HashSet::new()];
            let mut in_string = false;
            let mut escape = false;
            let mut current_string = String::new();

            let bytes = json.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let b = bytes[i];
                if in_string {
                    if escape {
                        current_string.push(b as char);
                        escape = false;
                    } else if b == b'\\' {
                        escape = true;
                    } else if b == b'"' {
                        in_string = false;
                        let mut j = i + 1;
                        while j < bytes.len()
                            && (bytes[j] == b' '
                                || bytes[j] == b'\t'
                                || bytes[j] == b'\r'
                                || bytes[j] == b'\n')
                        {
                            j += 1;
                        }
                        if j < bytes.len() && bytes[j] == b':' {
                            if let Some(current_set) = stack.last_mut() {
                                if !current_set.insert(current_string.clone()) {
                                    duplicates.push(format!("{file_name}: duplicate key \"{}\"", current_string));
                                }
                            }
                        }
                        current_string.clear();
                    } else {
                        current_string.push(b as char);
                    }
                } else {
                    match b {
                        b'"' => in_string = true,
                        b'{' => stack.push(std::collections::HashSet::new()),
                        b'}' => {
                            stack.pop();
                        }
                        _ => {}
                    }
                }
                i += 1;
            }

            assert!(
                duplicates.is_empty(),
                "Found duplicate keys in {}:\n{}",
                file_name,
                duplicates.join("\n")
            );
        }

        check_json(ZH_JSON, "zh.json");
        check_json(EN_JSON, "en.json");
    }

    #[test]
    fn test_no_deprecated_keys() {
        let zh_map = load_translations(ZH_JSON);
        let deprecated_prefixes = ["sidebar.", "settings.font_family", "settings.line_height", "settings.ui_font_size"];
        let mut found = Vec::new();
        for k in zh_map.keys() {
            for dep in &deprecated_prefixes {
                if k.starts_with(dep) {
                    found.push(k.clone());
                }
            }
        }
        assert!(found.is_empty(), "Deprecated keys found in locale:\n{:?}", found);
    }

    #[test]
    fn test_zh_punctuation_lint() {
        let zh_map = load_translations(ZH_JSON);
        let mut errors = Vec::new();
        for (k, v) in zh_map.iter() {
            if v.contains("...") && !v.contains("sk-...") {
                errors.push(format!("Key '{k}' contains ASCII '...' instead of '…': {v}"));
            }
        }
        assert!(errors.is_empty(), "Chinese punctuation errors found:\n{}", errors.join("\n"));
    }

    #[test]
    fn test_common_action_and_state_hierarchy() {
        let store = TranslationStore::new();
        assert_eq!(store.translate(Locale::Zh, "common.action.save"), "保存");
        assert_eq!(store.translate(Locale::En, "common.action.save"), "Save");
        assert_eq!(store.translate(Locale::Zh, "common.action.cancel"), "取消");
        assert_eq!(store.translate(Locale::En, "common.action.cancel"), "Cancel");
        assert_eq!(store.translate(Locale::Zh, "common.state.loading"), "加载中…");
        assert_eq!(store.translate(Locale::En, "common.state.loading"), "Loading...");
        assert_eq!(store.translate(Locale::Zh, "common.status.running"), "运行中");
        assert_eq!(store.translate(Locale::En, "common.status.running"), "Running");
        assert_eq!(store.translate(Locale::Zh, "common.navigation.back"), "返回");
        assert_eq!(store.translate(Locale::En, "common.navigation.back"), "Back");
    }

    #[test]
    fn test_terminology_consistency() {
        let zh_map = load_translations(ZH_JSON);
        let mut errors = Vec::new();
        for (k, v) in zh_map.iter() {
            if v.contains("复制渠道") {
                errors.push(format!("Key '{k}' uses improper term '复制渠道', expected '复制通道'"));
            }
            if v.contains("Agent 智能体") {
                errors.push(format!("Key '{k}' contains redundant 'Agent 智能体', expected 'Agent' or '智能体'"));
            }
        }
        assert!(errors.is_empty(), "Terminology inconsistencies found:\n{}", errors.join("\n"));
    }

    #[test]
    fn test_session_delete_and_theme_keys() {
        let store = TranslationStore::new();
        assert_eq!(store.translate(Locale::Zh, "session.delete_title"), "删除会话");
        assert_eq!(store.translate(Locale::En, "session.delete_title"), "Delete Session");
        assert_eq!(store.translate(Locale::Zh, "session.delete_confirm"), "确定要删除「{name}」吗？");
        assert_eq!(store.translate(Locale::En, "session.delete_confirm"), "Are you sure you want to delete '{name}'?");
        assert_eq!(store.translate(Locale::Zh, "settings.color_theme.custom"), "自定义");
        assert_eq!(store.translate(Locale::En, "settings.color_theme.custom"), "Custom");
    }

    #[test]
    fn test_codebase_i18n_keys_validity() {
        use std::path::{Path, PathBuf};

        let zh_map = load_translations(ZH_JSON);
        let en_map = load_translations(EN_JSON);

        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let crates_dir = manifest_dir.parent().expect("crates directory not found");

        let re = regex::Regex::new(r#"i18n(?:_t|_fmt)?!\s*\(\s*(?:[^,\"]+,\s*)?\"([^\"]+)\""#).unwrap();

        fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if name != "target" && name != ".git" {
                            collect_rs_files(&path, out);
                        }
                    } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                        out.push(path);
                    }
                }
            }
        }

        let mut rs_files = Vec::new();
        collect_rs_files(crates_dir, &mut rs_files);

        let mut missing_keys = Vec::new();

        for file in rs_files {
            let content = match std::fs::read_to_string(&file) {
                Ok(c) => c,
                Err(_) => continue,
            };

            for (line_idx, line) in content.lines().enumerate() {
                for cap in re.captures_iter(line) {
                    let key = cap.get(1).unwrap().as_str();
                    let in_zh = zh_map.contains_key(key);
                    let in_en = en_map.contains_key(key);
                    if !in_zh || !in_en {
                        let rel_path = file.strip_prefix(crates_dir).unwrap_or(&file);
                        missing_keys.push(format!(
                            "{}:{}: key \"{}\" missing (zh: {}, en: {})",
                            rel_path.display(),
                            line_idx + 1,
                            key,
                            in_zh,
                            in_en
                        ));
                    }
                }
            }
        }

        assert!(
            missing_keys.is_empty(),
            "Found {} invalid/missing i18n keys referenced in code:\n{}",
            missing_keys.len(),
            missing_keys.join("\n")
        );
    }

    #[test]
    fn test_common_fallback_resolution() {
        let store = TranslationStore::new();
        // Exact match
        assert_eq!(store.translate(Locale::Zh, "common.action.save"), "保存");
        // Fallback from short prefix
        assert_eq!(store.translate(Locale::Zh, "action.save"), "保存");
        assert_eq!(store.translate(Locale::Zh, "status.running"), "运行中");
        // Fallback for single common word
        assert_eq!(store.translate(Locale::Zh, "save"), "保存");
        assert_eq!(store.translate(Locale::Zh, "cancel"), "取消");
        assert_eq!(store.translate(Locale::En, "cancel"), "Cancel");
    }

    #[test]
    fn test_commands_structure_and_categories() {
        let store = TranslationStore::new();
        let zh_map = load_translations(ZH_JSON);
        let en_map = load_translations(EN_JSON);

        // Verify categories exist
        let categories = [
            "global", "terminal", "fullscreen", "search", "navigation",
            "project", "session", "services", "layout", "window",
            "view", "panel", "git", "other",
        ];
        for cat in categories {
            let key = format!("commands.category.{}", cat);
            assert!(zh_map.contains_key(&key), "Missing zh category: {}", key);
            assert!(en_map.contains_key(&key), "Missing en category: {}", key);
        }

        // Verify command items have label and description
        assert_eq!(store.translate(Locale::Zh, "commands.quit.label"), "退出");
        assert_eq!(store.translate(Locale::En, "commands.quit.label"), "Quit");
        assert_eq!(store.translate(Locale::Zh, "commands.quit.description"), "退出 Velowork");
        assert_eq!(store.translate(Locale::En, "commands.quit.description"), "Quit Velowork");

        assert_eq!(store.translate(Locale::Zh, "commands.split_vertical.label"), "垂直分屏");
        assert_eq!(store.translate(Locale::En, "commands.split_vertical.label"), "Split Vertical");

        assert_eq!(store.translate(Locale::Zh, "commands.category.global"), "全局");
        assert_eq!(store.translate(Locale::En, "commands.category.global"), "Global");

        // Verify every commands.<id>.label has commands.<id>.description
        for k in zh_map.keys() {
            if let Some(id) = k.strip_prefix("commands.").and_then(|s| s.strip_suffix(".label")) {
                let desc_key = format!("commands.{}.description", id);
                assert!(zh_map.contains_key(&desc_key), "Missing zh desc for: {}", id);
                assert!(en_map.contains_key(&desc_key), "Missing en desc for: {}", id);
            }
        }
    }
}
