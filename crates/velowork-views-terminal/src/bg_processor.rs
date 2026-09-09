//! Fast preprocessing + on-disk caching for the terminal background image.
//!
//! **Dual-cache strategy.** A huge (4K/8K or multi-MB) source is "pre-baked"
//! exactly ONCE — the moment the user first sets the image (or at app startup
//! if no cache exists yet) — into a lightweight JPEG stored in the temp cache dir.
//! Every subsequent app launch / terminal open then loads ONLY that lightweight file:
//! reading + decoding it is ~1-5ms versus multi-second, multi-MB full-resolution decode.
//! VRAM also drops significantly. This is the only robust cure for "超大图片启动卡顿".

use anyhow::Result;
use image::imageops::FilterType;
use image::GenericImageView;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Longest side of the pre-baked background (unblurred), in pixels.
/// At 1920px (Full HD), the background is razor-sharp on HD/2K/4K displays
/// without blur while keeping the cached JPEG file to ~200-350KB.
pub const BG_UNBLURRED_MAX_WIDTH: u32 = 1920;

/// Longest side of the pre-baked background when Gaussian blur is active.
/// Blurred images remove high frequencies so 1280px is optimal and keeps
/// cached JPEG file size tiny (~80KB).
pub const BG_BLUR_MAX_WIDTH: u32 = 1280;

/// Default max width fallback.
pub const BG_MAX_WIDTH: u32 = BG_UNBLURRED_MAX_WIDTH;

/// Gaussian blur radius (sigma) baked into the pre-baked image when the blur
/// setting is enabled. Gives an even, silky frosted-glass texture.
pub const BG_BLUR_SIGMA: f32 = 15.0;

/// Directory holding the pre-baked cache JPEGs.
fn cache_dir() -> PathBuf {
    let dir = std::env::temp_dir()
        .join("velowork")
        .join("terminal_bg_cache");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// `(mtime_seconds, size_bytes)` of `src`, used to invalidate a stale cache
/// when the user edits / replaces the original image.
fn source_freshness(src: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(src).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((mtime, meta.len()))
}

/// Deterministic cache-file path for `src` + `max_width` + `blur`. Embeds the source path,
/// its mtime + size, max_width, and the blur flag, so a changed source (or a toggled blur
/// setting) yields a DIFFERENT cache filename that is automatically re-baked —
/// while an unchanged source always resolves to the same (already-baked) file.
fn get_cache_file_path(src: &Path, max_width: u32, blur: bool) -> PathBuf {
    let (mtime, size) = source_freshness(src).unwrap_or((0, 0));
    let mut hasher = DefaultHasher::new();
    src.to_string_lossy().hash(&mut hasher);
    mtime.hash(&mut hasher);
    size.hash(&mut hasher);
    max_width.hash(&mut hasher);
    blur.hash(&mut hasher);
    let key = hasher.finish();
    cache_dir().join(format!("bg_{:016x}.jpg", key))
}

/// **Dual-cache entry point.** Returns the path to a lightweight, pre-baked
/// JPEG for `src` + `max_width` + `blur`:
///
/// * **Cache hit** (source unchanged since it was baked): the file already
///   exists, so this returns instantly (<1ms) with no processing.
/// * **Cache miss** (only on first set, or after the source changed): the
///   full-resolution source is read, downsampled to `max_width`, Gaussian-blurred
///   with `blur_sigma` (0 = no blur), and re-encoded to a lightweight JPEG in the
///   cache dir — then that path is returned.
///
/// This is CPU-heavy on a miss only; callers MUST run it on a background thread
/// (inside `cx.background_executor().spawn`) so the UI render thread is never
/// stalled. Returns `Err` for unsupported formats (e.g. SVG, which `image`
/// can't open but GPUI can) so the caller falls back to decoding the original.
pub fn get_or_process_background(
    src_path: &Path,
    max_width: u32,
    blur_sigma: f32,
) -> Result<PathBuf> {
    let blur = blur_sigma > 0.0;
    let cache_path = get_cache_file_path(src_path, max_width, blur);

    // Cache hit: source unchanged since we baked it — zero processing, instant.
    if cache_path.exists() {
        return Ok(cache_path);
    }

    // Cache miss: downsample the full-res source, blur if requested, re-encode JPEG.
    let img = image::open(src_path)?;
    let (w, h) = img.dimensions();
    let longest = w.max(h);

    // Use CatmullRom for sharp edge preservation when unblurred, and Triangle for speed when blurred.
    let filter = if blur {
        FilterType::Triangle
    } else {
        FilterType::CatmullRom
    };

    let resized = if longest > max_width {
        let scale = max_width as f32 / longest as f32;
        let nw = (w as f32 * scale).max(1.0) as u32;
        let nh = (h as f32 * scale).max(1.0) as u32;
        img.resize(nw, nh, filter)
    } else {
        img
    };

    let processed = if blur_sigma > 0.0 {
        resized.blur(blur_sigma)
    } else {
        resized
    };

    let mut out: Vec<u8> = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut out);
        // Use 90% quality when unblurred to eliminate JPEG compression artifacts, and 85% when blurred.
        let quality = if blur { 85 } else { 90 };
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
        encoder.encode_image(&processed)?;
    }
    std::fs::write(&cache_path, &out)?;
    Ok(cache_path)
}
