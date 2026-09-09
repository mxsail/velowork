//! Global, singleton cache for the terminal background image.
//!
//! Decodes the configured background image exactly once (off the main thread)
//! into a GPU-ready `RenderImage`, then shares that single `Arc` with every
//! terminal pane. This avoids re-decoding the same file per tab and gives all
//! tabs an instant, zero-flicker paint once the decode finishes.

use gpui::prelude::*;
use gpui::{
    div, hsla, img, px, AnyElement, Animation, App, AppContext, Context, Div, Entity, Global, ImageSource,
    Img, IntoElement, ObjectFit, SharedString, SvgRenderer,
};
use gpui::{AnimationExt, ParentElement, Styled, StyledImage};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::terminal_view_settings;

/// Detailed reason for background image validation or decode failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundImageError {
    /// File does not exist at specified path.
    NotFound,
    /// Specified path is a directory, not a regular file.
    IsDirectory,
    /// Lack of read permission for the file.
    PermissionDenied,
    /// Unsupported image format or extension.
    UnsupportedFormat,
    /// File is corrupted or cannot be decoded.
    DecodeFailed,
}

/// Expand leading `~` in a file path to the user's home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    let trimmed = path.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from) {
            return home.join(rest);
        }
    } else if trimmed == "~"
        && let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from) {
            return home;
    }
    PathBuf::from(trimmed)
}

/// Validate image path before attempting background decode.
/// Checks existence, directory status, permissions, and format extension.
pub fn validate_image_path(raw_path: &str) -> Result<PathBuf, BackgroundImageError> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err(BackgroundImageError::NotFound);
    }
    let path = expand_tilde(trimmed);

    // 1. Existence check
    if !path.exists() {
        return Err(BackgroundImageError::NotFound);
    }

    // 2. Directory check
    if path.is_dir() {
        return Err(BackgroundImageError::IsDirectory);
    }

    // 3. Permission check
    if let Err(e) = std::fs::File::open(&path) {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            return Err(BackgroundImageError::PermissionDenied);
        }
        return Err(BackgroundImageError::PermissionDenied);
    }

    // 4. Format / extension check
    let is_supported = matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tiff" | "tif" | "ico" | "svg" | "pnm")
    );
    if !is_supported {
        return Err(BackgroundImageError::UnsupportedFormat);
    }

    Ok(path)
}

/// Shared, decoded terminal background image plus bookkeeping state.
///
/// Stored as a single GPUI model so every terminal pane can observe it and
/// re-render (for the fade-in) the moment the single global decode completes.
pub struct TerminalBackgroundCache {
    /// Path currently requested, mirrors `TerminalViewSettings::terminal_background_image`.
    path: Option<String>,
    /// Resolved absolute filesystem path (with `~` expanded).
    resolved_path: Option<PathBuf>,
    /// Whether the blur setting was active for the current decode (part of the
    /// cache key so toggling blur re-preprocesses rather than reusing a stale texture).
    blur: bool,
    /// Decoded, GPU-ready image shared by all terminal panes.
    image: Option<Arc<gpui::RenderImage>>,
    /// Specific error if validation or decoding failed.
    error: Option<BackgroundImageError>,
    /// A decode is in flight for the current path.
    loading: bool,
}

impl TerminalBackgroundCache {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            path: None,
            resolved_path: None,
            blur: false,
            image: None,
            error: None,
            loading: false,
        }
    }

    /// Currently decoded image, if any. Shared across all panes.
    pub fn image(&self) -> Option<Arc<gpui::RenderImage>> {
        self.image.clone()
    }

    /// Whether decoding failed for the current path.
    pub fn failed(&self) -> bool {
        self.error.is_some()
    }

    /// Specific error reason if validation or decoding failed.
    pub fn error(&self) -> Option<BackgroundImageError> {
        self.error
    }

    /// Currently configured raw path, if any.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Currently configured blur flag.
    pub fn blur(&self) -> bool {
        self.blur
    }

    /// Whether a decode is currently in flight for the current path.
    pub fn loading(&self) -> bool {
        self.loading
    }

    /// Idempotent: ensure the cache holds a decoded image for `path`.
    ///
    /// `blur` mirrors `TerminalViewSettings::terminal_background_image_blur`: it
    /// is part of the cache key, so toggling blur re-preprocesses the source
    /// instead of reusing a stale (unblurred / over-blurred) texture.
    ///
    /// Does nothing if the path+blur pair is unchanged and the result (decoded /
    /// failed / still loading) is already known. Re-triggers the off-thread
    /// decode only when the key changes or was never requested.
    pub fn ensure_loaded(
        &mut self,
        path: Option<String>,
        blur: bool,
        cx: &mut Context<Self>,
    ) {
        // Avoid redundant work (and spurious `cx.notify` -> re-render loops):
        // - No path configured: stable once our stored path is also `None`.
        // - A path configured: stable once the result is known or in flight,
        //   AND the blur key matches.
        let already_known = if path.is_none() {
            self.path.is_none()
        } else {
            self.path == path
                && self.blur == blur
                && (self.image.is_some() || self.error.is_some() || self.loading)
        };
        if already_known {
            return;
        }

        self.path = path.clone();
        self.blur = blur;
        self.image = None;
        self.error = None;
        self.loading = false;

        let Some(raw_path) = path else {
            self.resolved_path = None;
            cx.notify();
            return;
        };

        // Perform synchronous pre-validation
        let resolved = match validate_image_path(&raw_path) {
            Ok(p) => p,
            Err(err) => {
                self.resolved_path = None;
                self.error = Some(err);
                self.loading = false;
                cx.notify();
                return;
            }
        };

        self.resolved_path = Some(resolved.clone());
        self.loading = true;
        self.error = None;
        cx.notify();

        let bg = cx.background_executor().clone();
        let svg = cx.svg_renderer();
        let this = cx.entity();
        let path_for_decode = resolved.to_string_lossy().to_string();
        let req_path = raw_path.clone();
        let req_blur = blur;

        // Kick the CPU-heavy decode off synchronously on the background executor.
        let decode = bg.spawn(async move {
            let max_w = if blur {
                crate::bg_processor::BG_BLUR_MAX_WIDTH
            } else {
                crate::bg_processor::BG_UNBLURRED_MAX_WIDTH
            };
            let blur_sigma = if blur {
                crate::bg_processor::BG_BLUR_SIGMA
            } else {
                0.0
            };

            // Dual-cache: obtain the pre-baked lightweight JPEG path.
            let cache_path = match crate::bg_processor::get_or_process_background(
                std::path::Path::new(&path_for_decode),
                max_w,
                blur_sigma,
            ) {
                Ok(p) => p,
                Err(_) => {
                    let bytes = smol::fs::read(&path_for_decode).await.ok()?;
                    return decode_image(&path_for_decode, bytes, &svg);
                }
            };

            let bytes = smol::fs::read(&cache_path).await.ok()?;
            decode_jpeg_bytes(bytes, &svg)
        });

        cx.spawn(async move |_this, cx| {
            let result = decode.await;
            this.update(cx, |cache, cx| {
                // Guard against race condition: only apply if this decode matches current requested path & blur
                if cache.path.as_deref() == Some(&req_path) && cache.blur == req_blur {
                    cache.loading = false;
                    match result {
                        Some(image) => {
                            cache.image = Some(image);
                            cache.error = None;
                        }
                        None => {
                            cache.error = Some(BackgroundImageError::DecodeFailed);
                        }
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// Decode already-JPEG bytes (e.g. the output of `preprocess_terminal_background`)
/// into a GPU-ready `RenderImage` shared by every terminal pane.
pub fn decode_jpeg_bytes(
    bytes: Vec<u8>,
    svg: &gpui::SvgRenderer,
) -> Option<Arc<gpui::RenderImage>> {
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Jpeg, bytes);
    image.to_image_data(svg.clone()).ok()
}

/// Decode a local image file into a GPU-ready `RenderImage` on a background
/// thread. Returns `None` if the file can't be read or the format is unknown.
pub fn decode_image(
    path: &str,
    bytes: Vec<u8>,
    svg: &SvgRenderer,
) -> Option<Arc<gpui::RenderImage>> {
    let format = match std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => gpui::ImageFormat::Png,
        Some("jpg") | Some("jpeg") => gpui::ImageFormat::Jpeg,
        Some("webp") => gpui::ImageFormat::Webp,
        Some("gif") => gpui::ImageFormat::Gif,
        Some("bmp") => gpui::ImageFormat::Bmp,
        Some("tiff") | Some("tif") => gpui::ImageFormat::Tiff,
        Some("ico") => gpui::ImageFormat::Ico,
        Some("svg") => gpui::ImageFormat::Svg,
        Some("pnm") => gpui::ImageFormat::Pnm,
        _ => return None,
    };
    let image = gpui::Image::from_bytes(format, bytes);
    image.to_image_data(svg.clone()).ok()
}

/// Global handle to the shared terminal background cache.
pub struct GlobalTerminalBackgroundCache(pub Entity<TerminalBackgroundCache>);

impl Global for GlobalTerminalBackgroundCache {}

/// Create and register the global terminal background cache, then kick off the
/// decode of the currently configured image. Call once at app startup, **after**
/// both the extension settings store and `GlobalSettings` are initialized — the
/// path/blur are read from terminal-view settings (which resolve the current
/// value through `settings_entity` -> `GlobalSettings`).
///
/// The decode runs **asynchronously on a background thread** (via
/// `ensure_loaded`): the heavy downsample + blur + decode never blocks app
/// launch or the UI. Terminals subscribe to the cache and fade the image in the
/// instant the shared texture completes, so there is no decode-induced pop-in
/// and no startup stall regardless of how large the source image is. Runtime
/// path/blur changes also flow through `ensure_loaded`.
pub fn init_terminal_background_cache(cx: &mut App) {
    let cache = cx.new(TerminalBackgroundCache::new);

    let tvs = terminal_view_settings(cx);
    let path = tvs.terminal_background_image.clone();
    let blur = tvs.terminal_background_image_blur;
    cache.update(cx, |cache, cx| cache.ensure_loaded(path, blur, cx));

    cx.set_global(GlobalTerminalBackgroundCache(cache));
}

/// Access the shared terminal background cache entity. Returns `None` if the
/// global has not been initialized yet (e.g. in some tests).
pub fn terminal_background_cache(cx: &App) -> Option<Entity<TerminalBackgroundCache>> {
    cx.try_global::<GlobalTerminalBackgroundCache>()
        .map(|c| c.0.clone())
}

/// Build the terminal background layer element (custom image, if configured).
///
/// Returns `None` when no background image is configured, or while the shared
/// decode is still in flight. This is the single source of truth for "what the
/// terminal background looks like" and is reused by both the individual
/// `TerminalPane` and the center grid wrapper — so the center can paint a
/// full-area background that sizes immediately with layout, independent of the
/// lagging alacritty text/background texture. This mirrors Zed's Pane/Editor/
/// DisplayMap separation: the Pane (background) resizes with layout on the same
/// frame a dock collapses, while the Editor (terminal content) catches up one
/// frame later, unnoticed.
///
/// * `anim_id` — unique animation key (only used when `animate` is true).
/// * `animate` — fade the image in on first paint (used by individual panes).
///
/// Builds the terminal background image element with uniform corner radius applied
/// directly to both the image and the scrim wrapper, ensuring GPU texture shader
/// rounds all 4 corners to match the terminal card.
pub fn terminal_background_element(
    cx: &App,
    anim_id: SharedString,
    animate: bool,
    radius: f32,
) -> Option<AnyElement> {
    terminal_background_element_with_corners(cx, anim_id, animate, radius, false)
}

pub fn terminal_background_element_with_corners(
    cx: &App,
    anim_id: SharedString,
    animate: bool,
    radius: f32,
    bottom_only: bool,
) -> Option<AnyElement> {
    let tvs = terminal_view_settings(cx);
    let img_path = tvs.terminal_background_image.clone()?;

    let (bg_cache_image, bg_cache_failed) = match terminal_background_cache(cx) {
        Some(cache) => {
            let cache = cache.read(cx);
            (cache.image(), cache.failed())
        }
        None => (None, false),
    };

    let apply_corners_img = |base: Img| {
        base.when(radius > 0.0, |d| {
            if bottom_only {
                d.rounded_bl(px(radius)).rounded_br(px(radius))
            } else {
                d.rounded(px(radius))
            }
        })
    };

    let apply_corners_div = |base: Div| {
        base.when(radius > 0.0, |d| {
            if bottom_only {
                d.rounded_bl(px(radius)).rounded_br(px(radius))
            } else {
                d.rounded(px(radius))
            }
        })
    };

    let image_el: AnyElement = if let Some(arc) = bg_cache_image {
        let base = img(ImageSource::Render(arc))
            .absolute()
            .inset_0()
            .size_full()
            .object_fit(ObjectFit::Cover);
        let base = apply_corners_img(base);
        if animate {
            base.with_animation(
                anim_id,
                Animation::new(Duration::from_millis(280)).with_easing(|t| 1.0 - (1.0 - t).powi(3)),
                move |el, delta| el.opacity(delta),
            )
            .into_any_element()
        } else {
            base.into_any_element()
        }
    } else if bg_cache_failed {
        let base = img(PathBuf::from(img_path))
            .absolute()
            .inset_0()
            .size_full()
            .object_fit(ObjectFit::Cover);
        let base = apply_corners_img(base);
        base.into_any_element()
    } else {
        return None;
    };

    let mut wrapper = div()
        .absolute()
        .inset_0()
        .size_full()
        .overflow_hidden()
        .child(image_el);

    if tvs.terminal_background_image_blur {
        let scrim = apply_corners_div(
            div()
                .absolute()
                .inset_0()
                .size_full()
                .bg(hsla(0.0, 0.0, 0.0, 0.45)),
        );
        wrapper = wrapper.child(scrim);
    }

    Some(wrapper.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde() {
        let p = expand_tilde("~/test.png");
        assert!(!p.to_string_lossy().starts_with('~'));
        assert!(p.to_string_lossy().ends_with("test.png"));
    }

    #[test]
    fn test_validate_image_path_empty_or_nonexistent() {
        assert_eq!(validate_image_path(""), Err(BackgroundImageError::NotFound));
        assert_eq!(validate_image_path("   "), Err(BackgroundImageError::NotFound));
        assert_eq!(validate_image_path("/non/existent/path/bg_12345.png"), Err(BackgroundImageError::NotFound));
    }

    #[test]
    fn test_validate_image_path_directory() {
        // /tmp or current dir exists
        let tmp = std::env::temp_dir();
        assert_eq!(validate_image_path(&tmp.to_string_lossy()), Err(BackgroundImageError::IsDirectory));
    }

    #[test]
    fn test_validate_image_path_unsupported_format() {
        // Create a temporary non-image file
        let mut tmp_file = std::env::temp_dir();
        tmp_file.push("velowork_test_script.sh");
        let _ = std::fs::write(&tmp_file, b"echo hello");
        assert_eq!(validate_image_path(&tmp_file.to_string_lossy()), Err(BackgroundImageError::UnsupportedFormat));
        let _ = std::fs::remove_file(&tmp_file);
    }
}
