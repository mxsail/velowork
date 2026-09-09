//! 全局通用终端仿真类型（TERM）定义与辅助函数。
//!
//! 维护 Velowork 全局通用的标准终端仿真类型列表，并提供默认值与有效性校验。

/// 全局支持的标准终端仿真类型列表（按现代真彩、通用标准、多路复用、系统控制台与经典 DEC VT 硬件基准组织）。
pub const SUPPORTED_TERM_TYPES: &[&str] = &[
    "xterm-256color",
    "xterm-direct",
    "xterm",
    "xterm-color",
    "tmux-256color",
    "tmux",
    "screen-256color",
    "screen",
    "linux",
    "vt100",
    "vt102",
    "vt220",
    "ansi",
    "dumb",
];

/// 默认终端仿真类型
pub const DEFAULT_TERM_TYPE: &str = "xterm-256color";

/// 检查给定终端类型字符串是否在标准受支持的列表中（区分大小写，通常 TERM 为小写）。
pub fn is_supported_term_type(s: &str) -> bool {
    let trimmed = s.trim();
    SUPPORTED_TERM_TYPES.contains(&trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_term_types_contains_default() {
        assert!(SUPPORTED_TERM_TYPES.contains(&DEFAULT_TERM_TYPE));
        assert!(is_supported_term_type(DEFAULT_TERM_TYPE));
    }

    #[test]
    fn test_supported_term_types_count_and_uniqueness() {
        assert_eq!(SUPPORTED_TERM_TYPES.len(), 14);
        let mut set = std::collections::HashSet::new();
        for &item in SUPPORTED_TERM_TYPES {
            assert!(set.insert(item), "duplicate term type: {item}");
        }
    }

    #[test]
    fn test_is_supported_term_type() {
        assert!(is_supported_term_type("xterm-256color"));
        assert!(is_supported_term_type("  vt100  "));
        assert!(is_supported_term_type("tmux-256color"));
        assert!(!is_supported_term_type("nonexistent-term-xyz"));
    }
}
