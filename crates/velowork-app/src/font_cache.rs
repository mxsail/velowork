//! 全局系统字体异步预加载与缓存模块。
//!
//! 在 Windows 等平台上，`cx.text_system().all_font_names()` 需要通过 DirectWrite COM 接口
//! 扫描注册表和字体目录中成百上千款字体，同步调用耗时高达 500ms ~ 2000ms+。
//! 本模块在应用启动后移入后台线程静默预加载，前台打开设置或会话弹窗时直接读取内存缓存（0ms 延迟），
//! 彻底消除弹窗卡顿。若预热尚未完成，即时返回常用推荐字体列表，确保 UI 绝不阻塞。

use std::sync::{Arc, OnceLock};
use gpui::TextSystem;
use parking_lot::RwLock;

static FONT_CACHE: OnceLock<RwLock<Option<Vec<String>>>> = OnceLock::new();

fn cache() -> &'static RwLock<Option<Vec<String>>> {
    FONT_CACHE.get_or_init(|| RwLock::new(None))
}

/// 内置常用推荐字体列表（涵盖常用等宽编程字体、现代系统 UI 字体及中文字体）。
pub static COMMON_FONT_FAMILIES: &[&str] = &[
    "JetBrains Mono",
    "Segoe UI",
    "Segoe UI Variable Text",
    "Cascadia Code",
    "Cascadia Mono",
    "Consolas",
    "Fira Code",
    "Hack",
    "Source Code Pro",
    "Courier New",
    "Microsoft YaHei",
    "Microsoft YaHei UI",
    "SimSun",
    "SimHei",
    "PingFang SC",
    "SF Pro",
    "Menlo",
    "Monaco",
    "Ubuntu Mono",
    "Noto Sans Mono",
    "Noto Sans CJK SC",
];

/// 在独立后台线程静默预加载全部系统字体。
/// 应该在应用初始化（如 `main.rs` 注册内置字体后）及早调用。
pub fn preload_system_fonts(text_system: Arc<TextSystem>) {
    let _ = std::thread::Builder::new()
        .name("font-cache-preload".into())
        .stack_size(512 * 1024)
        .spawn(move || {
            let mut fonts = text_system.all_font_names();
            if fonts.is_empty() {
                fonts = COMMON_FONT_FAMILIES.iter().map(|s| s.to_string()).collect();
            }
            fonts.retain(|f| {
                let trimmed = f.trim();
                !trimmed.is_empty() && !trimmed.starts_with('.')
            });
            fonts.sort_by_key(|a| a.to_lowercase());
            fonts.dedup();

            *cache().write() = Some(fonts);
            log::info!("System font cache preloaded successfully in background");
        });
}

/// 获取系统字体列表。
/// 若后台预加载已完成，直接返回内存缓存（0ms）；若尚未完成，返回内建常用字体列表，
/// 严禁阻塞 UI 线程。
pub fn get_system_font_names() -> Vec<String> {
    if let Some(cached) = cache().read().as_ref() {
        return cached.clone();
    }
    COMMON_FONT_FAMILIES.iter().map(|s| s.to_string()).collect()
}

/// 检查字体缓存是否已经预加载就绪。
pub fn is_font_cache_ready() -> bool {
    cache().read().is_some()
}
