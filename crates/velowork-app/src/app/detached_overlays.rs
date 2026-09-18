//! Generic helper for opening any `Render + Focusable` entity in a separate OS
//! window.
//!
//! This is the single entry point for every "detached" window in the app —
//! detached terminals, detached dock panels, file/diff viewers, … — so the
//! window-options construction, the `SimpleRoot` wrap and the window-bounds
//! persistence live in exactly one place instead of being copy-pasted at each
//! call site.

use crate::settings::settings;
use crate::views::overlays::detached_overlay::DetachedOverlayView;
use gpui::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::overlay::CloseEvent;

use crate::simple_root::SimpleRoot as Root;

/// Per-window options for a detached overlay.
pub struct DetachedOverlayOptions {
    /// Initial window size when no persisted bounds exist yet.
    pub size: Size<Pixels>,
    /// Minimum window size.
    pub min_size: Size<Pixels>,
    /// Handler invoked when the OS requests the window to close. Dock panels use
    /// this to re-attach the content back into the main window instead of
    /// closing. When `None`, the window simply closes.
    pub on_close: Option<Arc<dyn Fn(&mut Window, &mut App)>>,
    /// Hide wrapper titlebar when wrapped content renders its own window header/chrome (e.g. DockPanel, DetachedTerminalView).
    pub hide_titlebar: bool,
}

/// Generic host view wrapping a panel entity so it can be passed into
/// `open_detached_overlay`.
pub struct DetachedHost<T> {
    pub inner: Entity<T>,
}

impl<T: Render + 'static> Render for DetachedHost<T> {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.inner.clone()
    }
}

impl<T: Focusable + 'static> Focusable for DetachedHost<T> {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.inner.read(cx).focus_handle(cx)
    }
}

pub struct DetachedHostCloseEvent;
impl CloseEvent for DetachedHostCloseEvent {
    fn is_close(&self) -> bool {
        true
    }
}
impl<T: 'static> EventEmitter<DetachedHostCloseEvent> for DetachedHost<T> {}

impl Default for DetachedOverlayOptions {
    fn default() -> Self {
        Self {
            size: size(px(1400.0), px(900.0)),
            min_size: size(px(400.0), px(300.0)),
            on_close: None,
            hide_titlebar: false,
        }
    }
}

pub(crate) struct DetachedEntry {
    handle: AnyWindowHandle,
    on_close: Option<Arc<dyn Fn(&mut Window, &mut App)>>,
}

/// Global registry tracking all active detached windows (terminals, dock panels, settings, logs).
/// Allows the app to safely close and re-attach all floating windows when the screen locks.
#[derive(Clone, Default)]
pub struct DetachedWindowsRegistry(pub(crate) Arc<parking_lot::Mutex<Vec<DetachedEntry>>>);

impl gpui::Global for DetachedWindowsRegistry {}

/// Close all detached standalone OS windows (terminals, dock panels, settings, logs).
/// Executes each window's `on_close` handler (which safely re-attaches terminals and dock panels
/// back to the main window layout) before invoking `window.remove_window()`.
pub fn close_all_detached_windows(cx: &mut App) {
    let reg_arc = cx.try_global::<DetachedWindowsRegistry>().map(|r| r.0.clone());
    if let Some(reg) = reg_arc {
        let entries = std::mem::take(&mut *reg.lock());
        for entry in entries {
            let on_close = entry.on_close;
            let _ = entry.handle.update(cx, |_, window, cx| {
                if let Some(ref handler) = on_close {
                    handler(window, cx);
                }
                window.remove_window();
            });
        }
    }
}

/// Open `build`-produced content in a fresh OS window with the app's standard
/// client-drawn chrome and shared window-bounds persistence.
///
/// `build` runs inside the new window's context so it can create entities that
/// live in that window. Whatever it returns (`Entity<DockPanel>` wrapped in
/// `DetachedHost`, `Entity<DetachedTerminalView>` wrapped in `DetachedHost`,
/// a file/diff viewer, …) is hosted by `DetachedOverlayView`.
///
/// `E` is the content's `CloseEvent` type; it must be `DetachedHostCloseEvent`
/// for content that doesn't emit one (terminals, dock panels).
pub fn open_detached_overlay<T, E>(
    title: impl Into<SharedString>,
    build: impl FnOnce(&mut Window, Entity<OverlayRegistry>, &mut App) -> Entity<T>,
    opts: DetachedOverlayOptions,
    cx: &mut App,
) -> Option<AnyWindowHandle>
where
    T: Render + Focusable + EventEmitter<E> + 'static,
    E: CloseEvent + 'static,
{
    let title = title.into();
    // Wrap `on_close` with an atomic flag to guarantee Exactly-Once execution,
    // avoiding duplicate dock insertion if both OS close and programmatic dismiss occur.
    let on_close = opts.on_close.map(|handler| {
        let executed = Arc::new(AtomicBool::new(false));
        let once_handler: Arc<dyn Fn(&mut Window, &mut App)> = Arc::new(move |window, cx| {
            if !executed.swap(true, Ordering::SeqCst) {
                handler(window, cx);
            }
        });
        once_handler
    });
    let hide_titlebar = opts.hide_titlebar;

    let window_bounds = match settings(cx).detached_overlay_bounds {
        Some(b) => {
            let bounds = Bounds {
                origin: Point::new(px(b.origin_x), px(b.origin_y)),
                size: Size {
                    width: px(b.width),
                    height: px(b.height),
                },
            };
            use velowork_workspace::settings::DetachedWindowState;
            match b.state {
                DetachedWindowState::Windowed => WindowBounds::Windowed(bounds),
                DetachedWindowState::Maximized => WindowBounds::Maximized(bounds),
                DetachedWindowState::Fullscreen => WindowBounds::Fullscreen(bounds),
            }
        }
        None => WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: opts.size,
        }),
    };

    let settings = crate::settings::settings_entity(cx).read(cx).settings.clone();
    let (titlebar, window_decorations) =
        crate::views::chrome::title_bar::window_decorations_and_titlebar(
            settings.titlebar_style,
            title.clone(),
        );

    let on_close_for_build = on_close.clone();
    let handle = cx.open_window(
        WindowOptions {
            titlebar,
            window_bounds: Some(window_bounds),
            is_resizable: true,
            window_decorations,
            window_min_size: Some(opts.min_size),
            // See `SimpleRoot` / `window_background_appearance`: request the
            // transparent surface up front so rounded corners don't show square
            // tips on KDE Plasma (Wayland).
            window_background: crate::settings::window_background_appearance(&settings),
            ..Default::default()
        },
        move |window, cx| {
            let overlay_registry = cx.new(|_| OverlayRegistry::new());
            OverlayRegistry::set_global(overlay_registry.clone(), cx);
            let content = build(window, overlay_registry.clone(), cx);
            let view = cx.new(|cx| {
                DetachedOverlayView::new(content, overlay_registry, title.clone(), on_close_for_build, hide_titlebar, window, cx)
            });
            cx.new(|cx| Root::new(view, window, cx))
        },
    )
    .ok()
    .map(|wh| wh.into());

    if let Some(any_handle) = handle {
        if let Some(reg) = cx.try_global::<DetachedWindowsRegistry>().map(|r| r.0.clone()) {
            let mut entries = reg.lock();
            entries.push(DetachedEntry {
                handle: any_handle,
                on_close,
            });
        }
    }

    handle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn test_close_all_detached_windows_safe(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            cx.set_global(DetachedWindowsRegistry::default());
            // Safe when empty
            close_all_detached_windows(cx);
            let reg = cx.global::<DetachedWindowsRegistry>();
            assert!(reg.0.lock().is_empty());
        });
    }

    #[gpui::test]
    fn test_on_close_once_only(_cx: &mut gpui::TestAppContext) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let executed = Arc::new(AtomicBool::new(false));
        let mut once_handler = {
            let executed = executed.clone();
            move || {
                if !executed.swap(true, Ordering::SeqCst) {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                }
            }
        };

        once_handler();
        once_handler();
        assert_eq!(counter.load(Ordering::SeqCst), 1, "Handler must be called only once");
    }
}
