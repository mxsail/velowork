#![recursion_limit = "4096"]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

// The entire UI/app layer (views, app coordinator, keybindings, action
// dispatch, logging, and the thin shim modules over the lower-level crates)
// lives in its own crate (`velowork-app`) to keep it off the binary's hot compile
// path. The binary is now a thin entry point: it owns only `assets` and the
// `smoke_tests`, plus the allocator/bootstrap glue below.
//
// `velowork-app` re-exports `velowork-app-core`'s `settings`/`workspace`
// modules, so references that used to be `crate::settings` / `crate::workspace`
// are now `velowork_app::settings` / `velowork_app::workspace`.
mod assets;
#[cfg(test)]
mod smoke_tests;

use gpui::*;
use velowork_app::simple_root::SimpleRoot as Root;

// Global allocator. glibc malloc fragments badly under velowork's high-churn,
// multi-threaded small-allocation workload, so we override it. When the dhat
// heap profiler is enabled we hand the global allocator over to dhat instead so
// it can record every allocation.
//
// Only one global allocator may exist, so the cfgs are mutually exclusive with
// precedence dhat > jemalloc(unix) > mimalloc:
//   - Unix  (Linux/macOS): jemalloc. Shares a small fixed set of arenas across
//     all threads (narenas:2, see MALLOC_CONF below) instead of mimalloc's
//     per-thread heaps, so velowork's ~90 threads stop each pinning their own
//     segments — the dominant per-thread RSS overhead (cut anon heap ~440→180 MB).
//   - Windows: mimalloc (jemalloc/tikv-jemalloc-sys doesn't build under MSVC).
//   - Unix with `--features mimalloc` and jemalloc off: mimalloc, for A/B.
#[cfg(all(unix, feature = "jemalloc", not(feature = "dhat-heap")))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(all(
    feature = "mimalloc",
    not(all(unix, feature = "jemalloc")),
    not(feature = "dhat-heap")
))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// jemalloc runtime tuning, read from this weak symbol at init:
//   narenas:2        — cap arenas at 2 (vs default 4*ncpu), so threads pack into
//                      fewer shared arenas instead of spreading live data across
//                      dozens of half-used ones (reduces RSS fragmentation).
//   dirty_decay_ms:500 / muzzy_decay_ms:0 — return freed pages to OS rapidly.
#[cfg(all(unix, feature = "jemalloc", not(feature = "dhat-heap")))]
#[allow(non_upper_case_globals)]
#[unsafe(export_name = "_rjem_malloc_conf")]
pub static MALLOC_CONF: &[u8] = b"narenas:2,dirty_decay_ms:0,muzzy_decay_ms:0,background_thread:false\0";
#[cfg(all(unix, feature = "jemalloc", not(feature = "dhat-heap")))]
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static malloc_conf: &[u8] = b"narenas:2,dirty_decay_ms:0,muzzy_decay_ms:0,background_thread:false\0";

// Heap profiler (opt-in via `--features dhat-heap`). When enabled, dhat's
// allocator wraps the system allocator to record every allocation; the
// `Profiler` guard created at the top of `main` writes `dhat-heap.json` on exit.
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Writes to both stderr and a rotating log file simultaneously.
struct TeeWriter {
    stderr: std::io::Stderr,
    file: velowork_core::logging::RotatingFileWriter,
}

impl std::io::Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = self.stderr.write_all(buf);
        self.file.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = self.stderr.flush();
        self.file.flush()
    }
}

use velowork_app::app::Velowork;
use crate::assets::{Assets, embedded_fonts};
use velowork_app::init;
use velowork_app::keybindings;
use velowork_app::keybindings::{
    About, NewWindow, Quit, ShowSettings, ShowCommandPalette, ShowThemeSelector,
    ShowKeybindings, ShowProfileManager,
};
use velowork_app::logging;
use velowork_app::settings::GlobalSettings;
use velowork_app::views::panels::toast::ToastManager;
use velowork_app::workspace::persistence;
use velowork_app::workspace::state::GlobalWorkspace;
use velowork_core::profiles;

/// Quit action handler - flushes pending saves before exiting
fn quit(_: &Quit, cx: &mut App) {
    // Flush pending settings save
    if let Some(gs) = cx.try_global::<GlobalSettings>() {
        gs.0.read(cx).flush_pending_save();
    }

    // Flush pending workspace save
    if let Some(gw) = cx.try_global::<GlobalWorkspace>()
        && let Err(e) = persistence::save_workspace(gw.0.read(cx).data()) {
            log::error!("Failed to flush workspace on quit: {}", e);
        }

    // Flush pending session store save
    if let Some(ss) = cx.try_global::<velowork_workspace::stores::GlobalSessionStore>() {
        ss.0.read(cx).flush_pending_save();
    }

    // Flush pending tunnel store save
    if let Some(ts) = cx.try_global::<velowork_workspace::stores::GlobalTunnelStore>() {
        ts.0.read(cx).flush_pending_save();
    }

    cx.quit();
}



/// Set up platform-specific application icon (macOS Dock / Linux XDG icon theme)
#[cfg(target_os = "macos")]
fn setup_platform_app_icon() {
    use std::ffi::c_void;

    #[allow(clashing_extern_declarations)]
    unsafe extern "C" {
        fn objc_getClass(name: *const u8) -> *mut c_void;
        fn sel_registerName(name: *const u8) -> *mut c_void;

        #[link_name = "objc_msgSend"]
        fn msg(obj: *mut c_void, sel: *mut c_void) -> *mut c_void;

        #[link_name = "objc_msgSend"]
        fn msg_id(obj: *mut c_void, sel: *mut c_void, a: *mut c_void) -> *mut c_void;

        #[link_name = "objc_msgSend"]
        fn msg_bytes_len(obj: *mut c_void, sel: *mut c_void, bytes: *const u8, len: usize) -> *mut c_void;
    }

    unsafe {
        let alloc = sel_registerName(b"alloc\0".as_ptr());
        let icon_png = include_bytes!("../assets/logo.png");
        let ns_data = msg_bytes_len(
            objc_getClass(b"NSData\0".as_ptr()),
            sel_registerName(b"dataWithBytes:length:\0".as_ptr()),
            icon_png.as_ptr(),
            icon_png.len(),
        );
        let ns_image = msg_id(
            msg(objc_getClass(b"NSImage\0".as_ptr()), alloc),
            sel_registerName(b"initWithData:\0".as_ptr()),
            ns_data,
        );
        if !ns_image.is_null() {
            let app = msg(
                objc_getClass(b"NSApplication\0".as_ptr()),
                sel_registerName(b"sharedApplication\0".as_ptr()),
            );
            msg_id(
                app,
                sel_registerName(b"setApplicationIconImage:\0".as_ptr()),
                ns_image,
            );
        }
    }
}

#[cfg(target_os = "linux")]
fn setup_platform_app_icon() {
    if let Some(home) = std::env::var_os("HOME") {
        let home_path = std::path::PathBuf::from(home);
        let data_dir = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home_path.join(".local").join("share"));

        let icons_dir = data_dir.join("icons").join("hicolor");
        let apps_desktop_dir = data_dir.join("applications");

        let png_sizes: &[(&str, &[u8])] = &[
            ("16x16", include_bytes!("../assets/app-icon-16.png")),
            ("24x24", include_bytes!("../assets/app-icon-24.png")),
            ("32x32", include_bytes!("../assets/app-icon-32.png")),
            ("48x48", include_bytes!("../assets/app-icon-48.png")),
            ("64x64", include_bytes!("../assets/app-icon-64.png")),
            ("128x128", include_bytes!("../assets/app-icon-128.png")),
            ("256x256", include_bytes!("../assets/app-icon-256.png")),
            ("512x512", include_bytes!("../assets/app-icon-512.png")),
        ];

        for (size, bytes) in png_sizes {
            let dir = icons_dir.join(size).join("apps");
            if std::fs::create_dir_all(&dir).is_ok() {
                let _ = std::fs::write(dir.join("velowork.png"), bytes);
            }
        }

        let scalable_dir = icons_dir.join("scalable").join("apps");
        if std::fs::create_dir_all(&scalable_dir).is_ok() {
            let svg_bytes = include_bytes!("../assets/velowork_icon.svg");
            let _ = std::fs::write(scalable_dir.join("velowork.svg"), svg_bytes);
        }

        if std::fs::create_dir_all(&apps_desktop_dir).is_ok() {
            let desktop_content = "\
[Desktop Entry]
Type=Application
Name=Velowork
Comment=Cross-platform terminal multiplexer
Exec=velowork
Icon=velowork
Terminal=false
Categories=Development;System;TerminalEmulator;
StartupWMClass=velowork
";
            let desktop_file = apps_desktop_dir.join("velowork.desktop");
            let _ = std::fs::write(desktop_file, desktop_content);
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn setup_platform_app_icon() {}


/// Set up macOS application menu
fn set_app_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "Velowork".into(),
            disabled: false,
            items: vec![
                MenuItem::action("About Velowork", About),
                MenuItem::separator(),
                MenuItem::action("Settings...", ShowSettings),
                MenuItem::action("Profiles...", ShowProfileManager),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Quit Velowork", Quit),
            ],
        },
        Menu {
            name: "Edit".into(),
            disabled: false,
            items: vec![
                MenuItem::os_action("Undo", velowork_app::keybindings::Copy, OsAction::Undo), // Using Copy as placeholder since we need an action
                MenuItem::os_action("Redo", velowork_app::keybindings::Copy, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action("Cut", velowork_app::keybindings::Copy, OsAction::Cut),
                MenuItem::os_action("Copy", velowork_app::keybindings::Copy, OsAction::Copy),
                MenuItem::os_action("Paste", velowork_app::keybindings::Paste, OsAction::Paste),
                MenuItem::os_action("Select All", velowork_app::keybindings::Copy, OsAction::SelectAll),
            ],
        },
        Menu {
            name: "View".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Command Palette", ShowCommandPalette),
                MenuItem::action("Select Theme", ShowThemeSelector),
                MenuItem::separator(),
                MenuItem::action("Keyboard Shortcuts", ShowKeybindings),
            ],
        },
        Menu {
            name: "Window".into(),
            disabled: false,
            items: vec![
                MenuItem::action("New Window", NewWindow),
            ],
        },
    ]);
}

fn main() {
    // Handle --version before initializing anything (used by updater validation)
    if std::env::args().any(|a| a == "--version") {
        println!("velowork {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Start heap profiling for the lifetime of the process. Held until `main`
    // returns, at which point dhat writes `dhat-heap.json` into the cwd.
    #[cfg(feature = "dhat-heap")]
    let _dhat = dhat::Profiler::new_heap();

    let args: Vec<String> = std::env::args().collect();

    // Parse Data Root / Config Root flags
    let portable_flag = args.iter().any(|a| a == "--portable");
    let config_root_flag: Option<std::path::PathBuf> = args
        .iter()
        .position(|a| {
            a == "--config-root"
                || a.starts_with("--config-root=")
                || a == "--data-dir"
                || a.starts_with("--data-dir=")
        })
        .and_then(|pos| {
            let a = &args[pos];
            if let Some(val) = a
                .strip_prefix("--config-root=")
                .or_else(|| a.strip_prefix("--data-dir="))
            {
                Some(std::path::PathBuf::from(val))
            } else {
                args.get(pos + 1).map(std::path::PathBuf::from)
            }
        });

    // Resolve Data Root & initialize singleton before anything else
    let data_root = match velowork_core::data_root::resolve_data_root(config_root_flag, portable_flag) {
        Ok(dr) => dr,
        Err(e) => {
            eprintln!("Failed to resolve data root: {e}");
            std::process::exit(1);
        }
    };
    velowork_core::data_root::init(data_root);

    // Handle --list-profiles before anything else
    if args.iter().any(|a| a == "--list-profiles") {
        profiles::list_profiles();
        return;
    }

    // Handle --new-profile <name>: create and launch with it
    let new_profile_name: Option<String> = args
        .iter()
        .position(|a| a == "--new-profile")
        .and_then(|pos| args.get(pos + 1).cloned());

    // Propagate the binary's version into velowork-terminal so XTVERSION
    // responses identify as `velowork(<version>)` rather than the library's
    // internal crate version.
    velowork_terminal::terminal::set_app_version(env!("CARGO_PKG_VERSION"));

    // Give PTYs and sockets FD headroom (macOS' 256 soft default is stingy for
    // a multiplexer). The command bus separately caps concurrent subprocesses.
    velowork_core::process::raise_fd_limit();

    // Parse --profile <id> (or --profile=<id>)
    let profile_flag: Option<String> = args
        .iter()
        .position(|a| a == "--profile" || a.starts_with("--profile="))
        .and_then(|pos| {
            let a = &args[pos];
            if let Some(val) = a.strip_prefix("--profile=") {
                Some(val.to_string())
            } else {
                args.get(pos + 1).cloned()
            }
        });

    // If --new-profile was given, create the profile first then launch with it
    let effective_flag = if let Some(name) = new_profile_name {
        match profiles::create_profile(&name) {
            Ok(id) => {
                eprintln!("Created profile '{}' (id: {})", name, id);
                Some(id)
            }
            Err(e) => {
                eprintln!("Failed to create profile: {e}");
                std::process::exit(1);
            }
        }
    } else {
        profile_flag
    };

    // Resolve the active profile and register it as the process-wide global.
    // This must happen before logging (which uses the profile's log path) and
    // before CLI subcommands (which use config_dir() → profile root).
    let profile_paths = match profiles::resolve_active_profile(effective_flag) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    // SAFETY: called before any threads are spawned; no concurrent reads of the environment.
    unsafe { std::env::set_var("VELOWORK_PROFILE", &profile_paths.id) };
    let profile_log = profile_paths.log_path();
    profiles::init_profile(profile_paths);

    // 创建 Data Root 全局日志与终端录制目录
    let _ = std::fs::create_dir_all(velowork_core::data_root::current().logs_dir());
    let _ = std::fs::create_dir_all(velowork_core::data_root::current().recordings_dir());

    // 创建 Profile 标准子目录
    let _ = std::fs::create_dir_all(profiles::current().data_dir());
    let _ = std::fs::create_dir_all(profiles::current().config_dir());
    let _ = std::fs::create_dir_all(profiles::current().themes_dir());
    let _ = std::fs::create_dir_all(profiles::current().sessions_dir());

    // 初始化单库业务数据库（data/velowork.db），供 Repository/Service 层使用。
    if let Err(e) =
        velowork_core::storage::init_database(&profiles::current().database_path())
    {
        eprintln!("Warning: failed to open profile database: {e}");
    }

    // Set up rolling file logging: append mode, size-based rotation and historical retention
    let log_target = (|| -> Option<env_logger::fmt::Target> {
        let max_size = velowork_core::logging::max_log_size_bytes_from_env();
        let max_files = velowork_core::logging::max_log_files_from_env();
        let file = velowork_core::logging::RotatingFileWriter::new(profile_log, max_size, max_files).ok()?;
        Some(env_logger::fmt::Target::Pipe(Box::new(TeeWriter {
            stderr: std::io::stderr(),
            file,
        })))
    })();

    // Build the effective filter: always capture errors and SlowGuard warnings
    // so freezes and panics land in velowork.log regardless of what the user has
    // in RUST_LOG. User's RUST_LOG is appended last so they can refine further.
    let user_filter = std::env::var("RUST_LOG").ok().unwrap_or_default();
    let effective_filter = if user_filter.is_empty() {
        "info,velowork_core::timing=warn".to_string()
    } else {
        format!("error,velowork_core::timing=warn,{user_filter}")
    };
    let mut builder = env_logger::Builder::new();
    builder.parse_filters(&effective_filter);
    if let Some(target) = log_target {
        builder.target(target);
    }
    // Wrap the env_logger sink so logs also feed the in-app log console's
    // in-memory ring + runtime-reloadable capture filter (see crate::logging).
    logging::init(builder.build());

    // Log panics to velowork.log (otherwise they only go to stderr which is lost)
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("PANIC: {}\n{}", info, backtrace);
        default_hook(info);
    }));

    // Acquire instance lock to prevent multiple Velowork processes from
    // clobbering each other's workspace.json.
    let _instance_lock = match persistence::acquire_instance_lock() {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    Application::with_platform(gpui_platform::current_platform(false)).with_assets(Assets).run(move |cx: &mut App| {
        // Quit the app when the last window is closed (default on macOS is to keep running)
        cx.set_quit_mode(QuitMode::LastWindowClosed);

        // Register action handlers for menu items
        cx.on_action(quit);

        // Set up macOS application menu
        set_app_menus(cx);

        // Set up platform application icon (macOS Dock / Linux XDG icon theme)
        setup_platform_app_icon();

        // Register embedded JetBrains Mono font
        #[allow(
            clippy::expect_used,
            reason = "embedded fonts ship with the binary — failure here means the build is broken"
        )]
        cx.text_system()
            .add_fonts(embedded_fonts())
            .expect("Failed to register embedded fonts");

        // Preload system fonts asynchronously in background to eliminate modal opening freeze
        velowork_app::font_cache::preload_system_fonts(cx.text_system().clone());

        // Register keybindings
        keybindings::register_keybindings(cx);

        // Initialize toast notification system
        cx.set_global(ToastManager::new());

        // Register extensions and the extension settings-store bridge.
        init::init_extensions(cx);

        // Initialize global settings + i18n, then load workspace and build theme.
        let (settings_entity, app_settings) = init::init_settings(cx);

        // Apply initial text antialiasing rendering mode
        let to_text_rendering_mode = |mode: velowork_workspace::settings::TextAntialiasingMode| match mode {
            velowork_workspace::settings::TextAntialiasingMode::PlatformDefault => gpui::TextRenderingMode::PlatformDefault,
            velowork_workspace::settings::TextAntialiasingMode::Subpixel => gpui::TextRenderingMode::Subpixel,
            velowork_workspace::settings::TextAntialiasingMode::Grayscale => gpui::TextRenderingMode::Grayscale,
        };
        cx.set_text_rendering_mode(to_text_rendering_mode(app_settings.text_antialiasing));

        // Observe text_antialiasing changes and apply them in real-time
        let settings_for_aa = settings_entity.clone();
        cx.observe(&settings_for_aa, move |entity, cx| {
            let mode = entity.read(cx).settings.text_antialiasing;
            cx.set_text_rendering_mode(to_text_rendering_mode(mode));
            cx.refresh_windows();
        })
        .detach();

        // Initialize updater service (sets GlobalUpdateInfo, starts background checker if enabled)
        init::init_updater(&app_settings, cx);
        // 延迟后台预热凭据后端（在应用窗口完全渲染 5 秒后再静默预热，或由首个会话按需装载），
        // 消除冷启动阶段建立 D-Bus 句柄引起的内存与 CPU 开销。
        let _ = std::thread::Builder::new()
            .name("credential-init".into())
            .stack_size(256 * 1024)
            .spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(5));
                let _ = velowork_workspace::secure_storage::credential_provider();
            });
        init::init_i18n(&app_settings, cx);
        let workspace_data = init::init_workspace(&app_settings, cx);
        let theme_entity = init::init_theme(&app_settings, cx);

        // Build the unified state layer (stores + AppState) and register the
        // cross-crate theme / UI-font / UI-scale providers.
        init::init_stores(cx, &settings_entity, &theme_entity);

        // Keep the global background opacity (theme.opacity) in sync with the
        // `bg_opacity` setting. Updating the theme entity + notify reuses the
        // existing theme re-render path, so every surface reading `surface_bg`
        // / `readable_text` refreshes when transparency changes.
        let settings_for_opacity = settings_entity.clone();
        let theme_for_opacity = theme_entity.clone();
        let settings_for_opacity_inner = settings_for_opacity.clone();
        cx.observe(&settings_for_opacity, move |_, cx| {
            let opacity = settings_for_opacity_inner.read(cx).settings.bg_opacity;
            theme_for_opacity.update(cx, |t, cx| {
                t.set_opacity(opacity);
                cx.notify();
            });
        })
        .detach();

        // Create PTY manager with session backend from settings
        let (pty_manager, pty_events) = init::init_pty(&app_settings);
        cx.set_global(velowork_app::GlobalPtyManager(pty_manager.clone()));

        // Keep global proxy settings in PtyManager synced with AppSettings changes
        let settings_for_proxy = settings_entity.clone();
        let pty_manager_for_proxy = pty_manager.clone();
        cx.observe(&settings_for_proxy, move |entity, _cx| {
            let s = &entity.read(_cx).settings;
            let mode_str = match s.proxy_mode {
                velowork_workspace::settings::ProxyMode::None => "none",
                velowork_workspace::settings::ProxyMode::System => "system",
                velowork_workspace::settings::ProxyMode::Http => "http",
            };
            pty_manager_for_proxy.set_global_proxy(velowork_terminal::GlobalProxySettings {
                mode: mode_str.to_string(),
                host: s.proxy_host.clone(),
                port: s.proxy_port,
            });
        })
        .detach();

        // 初始化全局同步状态存储（供状态栏 sync_status_btn 读取），需在启动引擎前。
        velowork_app::sync_engine::init_sync_status(cx);
        // 启动后台自动同步引擎（定时 + 配置变更触发），需在全局设置就绪后调用。
        velowork_app::sync_engine::start_sync_engine(cx);

        let (titlebar, window_decorations) =
            velowork_app::views::chrome::title_bar::window_decorations_and_titlebar(
                app_settings.titlebar_style,
                "Velowork",
            );

        #[allow(
            clippy::expect_used,
            reason = "main window creation failing at startup leaves nothing to recover into"
        )]
        let _ = cx.open_window(
            WindowOptions {
                titlebar,
                window_bounds: Some({
                    // Restore main window's last-known OS bounds so position
                    // (including which monitor) survives relaunch. Falls back
                    // to a default 1440x900 (16:10) at origin (0,0) on first
                    // launch or if the persisted bounds are absent — a wider
                    // aspect ratio feels more balanced for a multi-pane terminal.
                    let persisted = workspace_data.main_window.os_bounds;
                    if let Some(b) = persisted {
                        WindowBounds::Windowed(Bounds {
                            origin: Point { x: px(b.origin_x), y: px(b.origin_y) },
                            size: Size { width: px(b.width), height: px(b.height) },
                        })
                    } else {
                        WindowBounds::Windowed(Bounds {
                            origin: Point::default(),
                            size: size(px(1440.0), px(900.0)),
                        })
                    }
                }),
                is_resizable: true,
                window_decorations,
                window_min_size: Some(Size {
                    width: px(400.0),
                    height: px(300.0),
                }),
                app_id: Some("velowork".to_string()),
                // Match the runtime appearance negotiated by `SimpleRoot`: when
                // the custom titlebar + rounded corners (or transparency) is
                // active, request a transparent/blurred surface up front. On
                // KDE Plasma (Wayland) the alpha channel is locked at surface
                // creation, so this must be set here rather than only at runtime.
                window_background: velowork_app::settings::window_background_appearance(&app_settings),
                ..Default::default()
            },
            |window, cx| {
                // Detect initial system appearance
                let is_dark = matches!(
                    window.appearance(),
                    WindowAppearance::Dark | WindowAppearance::VibrantDark
                );
                theme_entity.update(cx, |theme, _cx| {
                    theme.set_system_appearance(is_dark);
                });

                // Set up appearance change observer
                let theme_for_observer = theme_entity.clone();
                window
                    .observe_window_appearance(move |window: &mut Window, cx: &mut App| {
                        let is_dark = matches!(
                            window.appearance(),
                            WindowAppearance::Dark | WindowAppearance::VibrantDark
                        );
                        theme_for_observer.update(cx, |theme, cx| {
                            theme.set_system_appearance(is_dark);
                            cx.notify();
                        });
                    })
                    .detach();

                // Wire up content pane registration so PTY events can notify terminal views
                velowork_views_terminal::set_register_content_pane_fn(Box::new(|terminal_id, weak_content| {
                    let mut registry = velowork_app::views::window::content_pane_registry().lock();
                    let panes = registry.entry(terminal_id).or_default();
                    // Re-layouts (e.g. workspace switch) re-register the same
                    // terminal, minting fresh panes. Drop dead weaks and skip an
                    // entity already present so the vec stays bounded by live
                    // viewers and a live pane isn't notified twice per PTY event.
                    let new_id = weak_content.entity_id();
                    panes.retain(|w| w.upgrade().is_some());
                    if !panes.iter().any(|w| w.entity_id() == new_id) {
                        panes.push(weak_content);
                    }
                }));

                // Create the main app view wrapped in Root
                let velowork = cx.new(|cx| {
                    Velowork::new(workspace_data, pty_manager.clone(), pty_events, window, cx)
                });
                cx.new(|cx| Root::new(velowork, window, cx))
            },
        )
        .expect("Failed to create main window");

        if std::env::var("VELOWORK_ACTIVATE").is_ok() {
            cx.activate(true);
        }

        // Post-bootstrap settle trim (single shot after ~1.5s window settle):
        // allows startup-transient objects (JSON parsing, themes, i18n tables, entity graph)
        // to complete their destructor cleanup, then requests the allocator (jemalloc / glibc)
        // to return unmapped/dirty pages back to the OS.
        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let before = velowork_core::memory::read_process_memory_stats();
            velowork_core::memory::trim_process_memory();
            let after = velowork_core::memory::read_process_memory_stats();

            if let (Some(b), Some(a)) = (before, after) {
                log::debug!(
                    "[memory:bootstrap] post-bootstrap trim completed | RSS: {:.1}MB -> {:.1}MB (delta: {:+.1}MB) | Anonymous: {:.1}MB -> {:.1}MB | File: {:.1}MB",
                    b.rss_kb as f64 / 1024.0,
                    a.rss_kb as f64 / 1024.0,
                    (a.rss_kb as i64 - b.rss_kb as i64) as f64 / 1024.0,
                    b.anonymous_kb as f64 / 1024.0,
                    a.anonymous_kb as f64 / 1024.0,
                    a.file_backed_kb as f64 / 1024.0,
                );
            }
        })
        .detach();

        // Flush pending saves on ALL quit paths (including window X button).
        // The Quit action handler only runs for Ctrl+Q / menu quit, not for
        // QuitMode::LastWindowClosed. on_app_quit fires for every exit path.
        let _quit_sub = cx.on_app_quit(|cx| {
            // Flush pending settings save
            if let Some(gs) = cx.try_global::<GlobalSettings>() {
                gs.0.read(cx).flush_pending_save();
            }

            // Flush pending workspace save
            if let Some(gw) = cx.try_global::<GlobalWorkspace>()
                && let Err(e) = persistence::save_workspace(gw.0.read(cx).data()) {
                    log::error!("Failed to flush workspace on quit: {}", e);
                }

            // Flush pending session store save
            if let Some(ss) = cx.try_global::<velowork_workspace::stores::GlobalSessionStore>() {
                ss.0.read(cx).flush_pending_save();
            }

            // Flush pending tunnel store save
            if let Some(ts) = cx.try_global::<velowork_workspace::stores::GlobalTunnelStore>() {
                ts.0.read(cx).flush_pending_save();
            }

            async {}
        });
    });
}
