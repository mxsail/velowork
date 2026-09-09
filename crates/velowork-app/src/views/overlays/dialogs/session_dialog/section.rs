//! 分组（Section）元数据与摘要类型。
//!
//! [`SectionDescriptor`] 是**纯元数据**：只含 id/标题/图标/默认展开/搜索关键字，
//! 不承载 render 函数——由 `SectionRegistry`（见 `mod.rs`）在运行时注册，
//! 从而支持插件在不修改核心的前提下新增分组。

use velowork_ui::icon::AppIcon;

/// 8 个分组，按 SSH 生命周期排序。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SshSection {
    Basic,
    Connection,
    Authentication,
    Terminal,
    Network,
    Security,
    Advanced,
    Notes,
}

impl SshSection {
    /// 默认顺序（导航与渲染共用）。
    pub const ALL: &'static [SshSection] = &[
        SshSection::Basic,
        SshSection::Connection,
        SshSection::Authentication,
        SshSection::Terminal,
        SshSection::Network,
        SshSection::Security,
        SshSection::Advanced,
        SshSection::Notes,
    ];

    pub const COUNT: usize = 8;

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// 根据协议类型获取可见的分组列表。
pub fn visible_sections(protocol: velowork_state::SessionProtocol) -> &'static [SshSection] {
    match protocol {
        velowork_state::SessionProtocol::Ssh => SshSection::ALL,
        velowork_state::SessionProtocol::Serial
        | velowork_state::SessionProtocol::Telnet
        | velowork_state::SessionProtocol::Local => &[
            SshSection::Basic,
            SshSection::Terminal,
            SshSection::Notes,
        ],
    }
}

/// 分组纯元数据描述符。
#[derive(Clone, Copy, Debug)]
pub struct SectionDescriptor {
    pub id: SshSection,
    pub title_key: &'static str,
    pub icon: AppIcon,
    pub default_expand: bool,
    pub search_keywords: &'static [&'static str],
}

impl SectionDescriptor {
    /// 关键字匹配（大小写不敏感，同时匹配标题 key 尾段）。
    pub fn matches_search(&self, keyword: &str) -> bool {
        let kw = keyword.trim().to_lowercase();
        if kw.is_empty() {
            return true;
        }
        self.search_keywords
            .iter()
            .any(|k| k.to_lowercase().contains(&kw))
            || self.title_key.to_lowercase().contains(&kw)
    }
}

/// 内建 8 组描述符（纯元数据，不含 render/advisor）。
pub const BUILTIN_SECTIONS: &[SectionDescriptor] = &[
    SectionDescriptor {
        id: SshSection::Basic,
        title_key: "ssh.section.basic",
        icon: AppIcon::Settings,
        default_expand: true,
        search_keywords: &[
            "basic", "name", "icon", "folder", "startup", "command",
            "基础", "常规", "名称", "启动命令", "图标", "文件夹", "目录",
        ],
    },
    SectionDescriptor {
        id: SshSection::Connection,
        title_key: "ssh.section.connection",
        icon: AppIcon::Link,
        default_expand: true,
        search_keywords: &[
            "connection", "host", "port", "proxy", "keepalive", "idle", "timeout",
            "username", "user", "addr", "address", "jump",
            "连接", "主机", "端口", "代理", "保活", "空闲", "超时", "用户名", "地址", "跳板机",
        ],
    },
    SectionDescriptor {
        id: SshSection::Authentication,
        title_key: "ssh.section.authentication",
        icon: AppIcon::EyeOff,
        default_expand: true,
        search_keywords: &[
            "auth", "password", "key", "agent", "passphrase", "private", "keyboard", "identity",
            "认证", "密码", "密钥", "口令", "私钥", "身份验证", "证书",
        ],
    },
    SectionDescriptor {
        id: SshSection::Terminal,
        title_key: "ssh.section.terminal",
        icon: AppIcon::Terminal,
        default_expand: true,
        search_keywords: &[
            "terminal", "term", "charset", "scrollback", "font", "shell",
            "integration", "paste", "osc52", "color", "encoding", "buffer",
            "终端", "字符集", "回滚", "集成", "字体", "编码", "缓冲区", "剪贴板",
        ],
    },
    SectionDescriptor {
        id: SshSection::Network,
        title_key: "ssh.section.network",
        icon: AppIcon::Transfer,
        default_expand: false,
        search_keywords: &[
            "network", "performance", "window", "packet", "nodelay", "tcp",
            "网络", "性能", "窗口", "延迟", "数据包",
        ],
    },
    SectionDescriptor {
        id: SshSection::Security,
        title_key: "ssh.section.security",
        icon: AppIcon::Check,
        default_expand: false,
        search_keywords: &[
            "security", "algorithm", "kex", "cipher", "mac", "hostkey", "compression", "strict",
            "安全", "算法", "压缩", "加密", "主机密钥", "指纹",
        ],
    },
    SectionDescriptor {
        id: SshSection::Advanced,
        title_key: "ssh.section.advanced",
        icon: AppIcon::Settings,
        default_expand: false,
        search_keywords: &[
            "advanced", "gex", "rekey", "compression", "min", "max", "preferred",
            "高级", "重协商", "算法配置",
        ],
    },
    SectionDescriptor {
        id: SshSection::Notes,
        title_key: "ssh.section.notes",
        icon: AppIcon::Folder,
        default_expand: false,
        search_keywords: &[
            "notes", "tags", "description", "remark",
            "备注", "标签", "描述", "说明", "注释",
        ],
    },
];
