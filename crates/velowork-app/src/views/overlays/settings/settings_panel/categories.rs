use velowork_i18n::i18n;

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum SettingsCategory {
    // Original categories (kept)
    General,
    Font,
    Terminal,
    Extensions,

    // New categories from prototype design
    Appearance,
    FileManager,
    Security,
    Sync,
    AiAssistant,

    /// User-configured search engines for the terminal "Search Online" action.
    SearchEngines,

    /// Data and storage path configuration (Data Root / Logs / Recordings).
    DataStorage,

    /// Dynamic category for an extension's own settings (keyed by extension ID).
    Extension(String),
}

impl SettingsCategory {
    pub(super) fn label(&self, cx: &gpui::App) -> String {
        match self {
            // Original categories
            Self::General => i18n!(cx, "settings.nav.general"),
            Self::Font => i18n!(cx, "settings.nav.font"),
            Self::Terminal => i18n!(cx, "settings.nav.terminal"),
            Self::Extensions => i18n!(cx, "settings.nav.extensions"),

            // New categories
            Self::Appearance => i18n!(cx, "settings.nav.appearance"),
            Self::FileManager => i18n!(cx, "settings.nav.file_manager"),
            Self::Security => i18n!(cx, "settings.nav.security"),
            Self::Sync => i18n!(cx, "settings.nav.sync"),
            Self::AiAssistant => i18n!(cx, "settings.nav.ai_assistant"),

            Self::SearchEngines => i18n!(cx, "settings.nav.search_engines"),

            Self::DataStorage => i18n!(cx, "settings.nav.data_storage"),

            Self::Extension(_) => String::new(),
        }
    }

    pub(super) fn all() -> &'static [SettingsCategory] {
        &[
            // Main categories (matching prototype design order)
            Self::General,
            Self::Appearance,
            Self::Font,
            Self::Terminal,
            Self::FileManager,
            Self::Security,
            Self::Sync,
            Self::AiAssistant,
            Self::SearchEngines,
            Self::DataStorage,
            // Advanced categories
            Self::Extensions,
        ]
    }

    pub(super) fn icon(&self) -> velowork_ui::icon::AppIcon {
        use velowork_ui::icon::AppIcon;
        match self {
            Self::General => AppIcon::Settings,
            Self::Appearance => AppIcon::PaintRoller,
            Self::Font => AppIcon::Edit,
            Self::Terminal => AppIcon::Terminal,
            Self::FileManager => AppIcon::Folder,
            Self::Security => AppIcon::Shield,
            Self::Sync => AppIcon::Cloud,
            Self::AiAssistant => AppIcon::AiAssistant,
            Self::SearchEngines => AppIcon::Search,
            Self::DataStorage => AppIcon::Database,
            Self::Extensions | Self::Extension(_) => AppIcon::CommandAction,
        }
    }

    /// 各分类包含的具体配置项中英文检索关键字，支持深度定位搜索
    pub(super) fn search_keywords(&self) -> &'static [&'static str] {
        match self {
            Self::General => &[
                "general", "language", "locale", "startup", "boot", "update", "close", "proxy", "http", "notification", "bell", "osc",
                "常规", "通用", "语言", "自启", "开机", "检查更新", "关闭行为", "网络代理", "代理", "通知", "提示音",
            ],
            Self::Appearance => &[
                "appearance", "theme", "dark", "light", "color", "palette", "density", "tab", "titlebar", "window", "radius",
                "外观", "主题", "深色", "浅色", "调色板", "紧凑", "密度", "标签页", "标题栏", "圆角", "窗口",
            ],
            Self::Font => &[
                "font", "family", "size", "line", "height", "weight", "monospace",
                "字体", "字号", "行高", "字重", "等宽", "UI字体",
            ],
            Self::Terminal => &[
                "terminal", "shell", "cursor", "blink", "bell", "charset", "color", "scheme", "encoding", "scrollback", "bg", "image", "pty",
                "终端", "外壳", "光标", "闪烁", "铃声", "字符集", "编码", "配色", "回滚", "背景图", "缓冲区",
            ],
            Self::FileManager => &[
                "file", "manager", "sftp", "permission", "mode", "sort", "hidden", "editor", "opener",
                "文件", "文件管理", "传输", "默认权限", "排序", "隐藏文件", "打开方式", "编辑器",
            ],
            Self::Security => &[
                "security", "master", "password", "lock", "idle", "timeout", "keyring",
                "安全", "主密码", "锁屏", "自动锁定", "空闲", "密钥环",
            ],
            Self::Sync => &[
                "sync", "cloud", "webdav", "s3", "minio", "r2", "oss", "cos", "bucket", "remote", "provider", "backup", "restore",
                "同步", "网盘", "云同步", "对象存储", "存储桶", "远程", "备份", "恢复", "凭证",
            ],
            Self::AiAssistant => &[
                "ai", "assistant", "model", "token", "context", "history", "provider", "openai", "claude", "ollama",
                "人工智能", "助手", "模型", "提示词", "上下文", "历史消息", "API密钥",
            ],
            Self::SearchEngines => &[
                "search", "engine", "online", "url", "query", "google", "bing", "baidu",
                "搜索", "搜索引擎", "在线搜索", "网址", "谷歌", "百度",
            ],
            Self::DataStorage => &[
                "data", "storage", "root", "path", "recording", "log", "export", "import", "cleanup",
                "数据", "存储", "数据目录", "录屏", "日志", "导出", "导入", "清理",
            ],
            Self::Extensions | Self::Extension(_) => &[
                "extension", "plugin", "addon", "marketplace",
                "扩展", "插件", "组件",
            ],
        }
    }

    /// 检查指定关键字是否匹配该分类（大小写不敏感，匹配标题或关键字列表）
    pub(super) fn matches_search(&self, keyword: &str, cx: &gpui::App) -> bool {
        let kw = keyword.trim().to_lowercase();
        if kw.is_empty() {
            return true;
        }
        if self.label(cx).to_lowercase().contains(&kw) {
            return true;
        }
        self.search_keywords()
            .iter()
            .any(|k| k.to_lowercase().contains(&kw))
    }

    /// 设置弹窗打开时默认展开的分类（仅 General 展开，其余折叠为卡片头）。
    pub(super) fn default_expand(&self) -> bool {
        matches!(self, SettingsCategory::General)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn test_category_metadata_and_search(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            velowork_i18n::init_locale(velowork_i18n::Locale::Zh, cx);

            for cat in SettingsCategory::all() {
                // 确保所有分类均配置有效图标与非空关键字
                let _icon = cat.icon();
                assert!(!cat.search_keywords().is_empty(), "category {:?} should have keywords", cat.label(cx));
            }

            // 深度配置项检索测试（中英文混合匹配）
            assert!(SettingsCategory::Sync.matches_search("webdav", cx));
            assert!(SettingsCategory::Sync.matches_search("云同步", cx));
            assert!(SettingsCategory::Terminal.matches_search("cursor", cx));
            assert!(SettingsCategory::Terminal.matches_search("光标", cx));
            assert!(SettingsCategory::Font.matches_search("monospace", cx));
            assert!(SettingsCategory::General.matches_search("proxy", cx));
            assert!(SettingsCategory::Security.matches_search("password", cx));

            // 空白关键字全量匹配
            assert!(SettingsCategory::General.matches_search("", cx));
            assert!(SettingsCategory::General.matches_search("   ", cx));

            // 不相关关键字不应误匹配
            assert!(!SettingsCategory::Font.matches_search("nonexistent_random_key_xyz", cx));
        });
    }
}
