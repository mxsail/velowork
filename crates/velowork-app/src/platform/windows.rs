//! Windows DWM and window frame management for VeloWork.
//!
//! Provides DWM frame extension (`DwmExtendFrameIntoClientArea`) to ensure
//! DirectComposition premultiplied alpha surfaces participate properly in
//! Windows desktop composition.

#[cfg(target_os = "windows")]
use windows::{
    core::PCSTR,
    Win32::{
        Foundation::{BOOL, HWND, LPARAM},
        Graphics::Dwm::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWINDOWATTRIBUTE},
        System::{
            LibraryLoader::{GetModuleHandleA, GetProcAddress},
            Threading::GetCurrentThreadId,
        },
        UI::{
            Controls::MARGINS,
            WindowsAndMessaging::{
                EnumThreadWindows, GetClassNameW, SetClassLongPtrW, SetWindowPos,
                GCLP_HBRBACKGROUND, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
            },
        },
    },
};

#[cfg(target_os = "windows")]
#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct WindowCompositionAttribData {
    attrib: u32,
    pv_data: *mut std::ffi::c_void,
    cb_data: usize,
}

/// Explicitly disable and clear any legacy accent policy on the window.
///
/// Upstream GPUI's `set_background_appearance(Transparent)` invokes
/// `SetWindowCompositionAttribute` with `ACCENT_ENABLE_TRANSPARENTGRADIENT` (state 2)
/// and `gradient_color = 0` (black). On Windows 11, this undocumented API forces
/// DWM to composite a solid black gradient behind the window.
///
/// Clearing this accent policy (setting `ACCENT_DISABLED` state 0) allows
/// DirectComposition and DWM frame composition to blend transparently with the desktop.
#[cfg(target_os = "windows")]
pub fn clear_accent_policy(hwnd: HWND) {
    unsafe {
        type SetWindowCompositionAttributeType =
            unsafe extern "system" fn(HWND, *mut WindowCompositionAttribData) -> BOOL;
        let module_name = PCSTR::from_raw(c"user32.dll".as_ptr() as *const u8);
        if let Ok(user32) = GetModuleHandleA(module_name) {
            let func_name = PCSTR::from_raw(c"SetWindowCompositionAttribute".as_ptr() as *const u8);
            if let Some(raw_proc) = GetProcAddress(user32, func_name) {
                let set_attr: SetWindowCompositionAttributeType = std::mem::transmute(raw_proc);
                let mut accent = AccentPolicy {
                    accent_state: 0, // ACCENT_DISABLED
                    accent_flags: 0,
                    gradient_color: 0,
                    animation_id: 0,
                };
                let mut data = WindowCompositionAttribData {
                    attrib: 0x13, // WCA_ACCENT_POLICY
                    pv_data: &mut accent as *mut _ as *mut std::ffi::c_void,
                    cb_data: std::mem::size_of::<AccentPolicy>(),
                };
                let res = set_attr(hwnd, &mut data);
                log::info!("[windows-dwm] clear_accent_policy ACCENT_DISABLED (state=0) result: {:?}", res);
            }
        }
    }
}

/// Configure DWM client area frame extension for transparency.
///
/// Calling `DwmExtendFrameIntoClientArea` with margin `-1` instructs DWM to
/// treat the client area as participating in unified DWM frame composition,
/// allowing DirectComposition transparent surfaces to blend with the desktop.
#[cfg(target_os = "windows")]
pub fn extend_frame_into_client_area(
    hwnd: HWND,
    transparent: bool,
    is_dark: bool,
    is_custom_titlebar: bool,
) {
    // When custom titlebar is active, NEVER extend the frame into the client area (keep margins at 0).
    // Calling DwmExtendFrameIntoClientArea with negative margins (especially cyTopHeight: -1) instructs
    // DWM to treat the client area top as the window caption, which causes DWM to render a duplicate
    // set of native system caption buttons (minimize, maximize, close) on top of VeloWork's custom titlebar!
    // On Windows 11 (22H2+), DWMWA_SYSTEMBACKDROP_TYPE (Desktop Acrylic) renders natively behind the
    // DirectComposition transparent surface without requiring DwmExtendFrameIntoClientArea.
    let margins = if transparent && !is_custom_titlebar {
        MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        }
    } else {
        MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 0,
            cyBottomHeight: 0,
        }
    };

    unsafe {
        let res = DwmExtendFrameIntoClientArea(hwnd, &margins);
        log::info!(
            "[windows-dwm] DwmExtendFrameIntoClientArea(hwnd={:?}, transparent={}, is_dark={}, custom_titlebar={}, margins={:?}): {:?}",
            hwnd,
            transparent,
            is_dark,
            is_custom_titlebar,
            margins,
            res
        );

        // Synchronize DWM Immersive Dark Mode with active theme so Acrylic frosted glass
        // renders with matching dark/light tint rather than washing out text contrast.
        let dark_mode = BOOL(if is_dark { 1 } else { 0 });
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(20), // DWMWA_USE_IMMERSIVE_DARK_MODE
            &dark_mode as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<BOOL>() as u32,
        );

        // Restore standard DWM non-client rendering policy (DWMNCRP_USEWINDOWSTYLE = 0).
        // CRITICAL FIX: Never set DWMNCRP_DISABLED (1)! When non-client rendering is disabled,
        // DWM stops handling non-client resizing borders and rounded corners, causing Windows
        // to fall back to legacy thick GDI rectangular frames (~8-10px) on the left, right,
        // and bottom edges.
        let ncrp: u32 = 0; // DWMNCRP_USEWINDOWSTYLE
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(2), // DWMWA_NCRENDERING_POLICY
            &ncrp as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );

        // Request Windows 11 native rounded corners (DWMWA_WINDOW_CORNER_PREFERENCE = 33, DWMWCP_ROUND = 2)
        let corner_preference: u32 = 2; // DWMWCP_ROUND
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(33), // DWMWA_WINDOW_CORNER_PREFERENCE
            &corner_preference as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );

        // When using a custom titlebar, suppress native caption drawing completely
        // (DWMWA_CAPTION_COLOR = 35, DWMWA_COLOR_NONE = 0xFFFFFFFE) to guarantee zero duplicate buttons.
        if is_custom_titlebar {
            let color_none: u32 = 0xFFFFFFFE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(35), // DWMWA_CAPTION_COLOR
                &color_none as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }

        if transparent {
            // Remove window class solid black brush so DWM/GDI doesn't erase background to black
            let _ = SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, 0);
            // Clear GPUI's legacy SetWindowCompositionAttribute (state = 2, color = 0)
            clear_accent_policy(hwnd);

            // On Windows 11 (22H2+), set DWMWA_SYSTEMBACKDROP_TYPE to DWMSBT_TRANSIENTWINDOW (3, Desktop Acrylic)
            // so DWM renders translucent frosted glass across the extended frame instead of falling back
            // to DWMSBT_NONE (1, which forces an opaque system-colored backplate).
            let backdrop_type: u32 = 3; // DWMSBT_TRANSIENTWINDOW (Desktop Acrylic)
            let dres = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(38),
                &backdrop_type as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
            log::info!(
                "[windows-dwm] DwmSetWindowAttribute DWMSBT_TRANSIENTWINDOW (3, Acrylic) result: {:?}",
                dres
            );
        } else {
            let backdrop_type: u32 = 1; // DWMSBT_NONE (Opaque)
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(38),
                &backdrop_type as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }

        // Notify Windows that window frame attributes have changed so non-client chrome is refreshed
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

/// Enumerate windows created on the current UI thread and apply DWM frame configuration.
#[cfg(target_os = "windows")]
pub fn sync_active_window_dwm_frame(transparent: bool, is_dark: bool, is_custom_titlebar: bool) {
    struct EnumParams {
        transparent: bool,
        is_dark: bool,
        is_custom_titlebar: bool,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let mut class_name = [0u16; 64];
        let len = unsafe { GetClassNameW(hwnd, &mut class_name) };
        if len > 0 {
            let name = String::from_utf16_lossy(&class_name[..len as usize]);
            if name == "Zed::Window" {
                let params = unsafe { &*(lparam.0 as *const EnumParams) };
                log::info!("[windows-dwm] sync_active_window_dwm_frame found Zed::Window: {:?}", hwnd);
                extend_frame_into_client_area(
                    hwnd,
                    params.transparent,
                    params.is_dark,
                    params.is_custom_titlebar,
                );
            }
        }
        BOOL(1)
    }

    let params = EnumParams {
        transparent,
        is_dark,
        is_custom_titlebar,
    };
    unsafe {
        let thread_id = GetCurrentThreadId();
        let _ = EnumThreadWindows(
            thread_id,
            Some(enum_proc),
            LPARAM(&params as *const _ as isize),
        );
    }
}

/// Synchronize DWM frame configuration directly using the given GPUI Window.
#[cfg(target_os = "windows")]
pub fn sync_window_dwm_frame(
    window: &gpui::Window,
    transparent: bool,
    is_dark: bool,
    is_custom_titlebar: bool,
) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::Win32(win32) = handle.as_raw() {
            let hwnd = HWND(win32.hwnd.get() as _);
            log::info!(
                "[windows-dwm] sync_window_dwm_frame directly for HWND {:?}, transparent: {}, is_dark: {}, custom_titlebar: {}",
                hwnd,
                transparent,
                is_dark,
                is_custom_titlebar
            );
            extend_frame_into_client_area(hwnd, transparent, is_dark, is_custom_titlebar);
            return;
        }
    }
    // Fallback to thread window enumeration
    sync_active_window_dwm_frame(transparent, is_dark, is_custom_titlebar);
}

/// Cross-platform entry point to synchronize DWM frame configuration.
#[cfg(not(target_os = "windows"))]
pub fn sync_window_dwm_frame(
    _window: &gpui::Window,
    _transparent: bool,
    _is_dark: bool,
    _is_custom_titlebar: bool,
) {
}

/// Cross-platform entry point to synchronize DWM frame configuration via thread windows.
#[cfg(not(target_os = "windows"))]
pub fn sync_active_window_dwm_frame(
    _transparent: bool,
    _is_dark: bool,
    _is_custom_titlebar: bool,
) {
}
