//! Velowork Brand Logo component with runtime anti-aliased SVG rasterization.
//!
//! Uses `resvg` + `tiny-skia` to rasterize `assets/velowork_icon.svg` into
//! pixel-perfect, anti-aliased RGBA images keyed by the physical display resolution
//! (`logical_pixels * window.scale_factor()`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use gpui::{App, Image, ImageFormat, ImageSource, Img, Pixels, RenderImage, Styled, SvgRenderer, Window, img};

/// Embedded raw SVG source of the Velowork brand logo.
pub static BRAND_LOGO_SVG: &[u8] = include_bytes!("../../../assets/velowork_icon.svg");

static LOGO_CACHE: OnceLock<Mutex<HashMap<u32, Arc<RenderImage>>>> = OnceLock::new();

fn get_logo_cache() -> &'static Mutex<HashMap<u32, Arc<RenderImage>>> {
    LOGO_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Rasterize the brand SVG to PNG bytes at an exact pixel size.
pub fn render_brand_logo_png(pixel_size: u32) -> Option<Vec<u8>> {
    if pixel_size == 0 {
        return None;
    }
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(BRAND_LOGO_SVG, &opt).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixel_size, pixel_size)?;

    let sx = pixel_size as f32 / tree.size().width();
    let sy = pixel_size as f32 / tree.size().height();
    let transform = resvg::tiny_skia::Transform::from_scale(sx, sy);

    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().ok()
}

/// Retrieve or render a `gpui::RenderImage` of the brand logo for the given physical pixel dimension.
pub fn get_or_render_brand_logo(pixel_size: u32, svg_renderer: &SvgRenderer) -> Option<Arc<RenderImage>> {
    let cache = get_logo_cache();
    {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(img) = guard.get(&pixel_size) {
            return Some(img.clone());
        }
    }

    let png_bytes = render_brand_logo_png(pixel_size)?;
    let image = Image::from_bytes(ImageFormat::Png, png_bytes);
    let render_img = image.to_image_data(svg_renderer.clone()).ok()?;

    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    guard.insert(pixel_size, render_img.clone());
    Some(render_img)
}

/// Create a `gpui::Img` element displaying the Velowork brand logo at the specified logical size.
///
/// Rasterized at `size * window.scale_factor()` so that gradients, shadows, and curves
/// are rendered with subpixel anti-aliasing without downsampling artifacts.
pub fn brand_logo(size: Pixels, window: &Window, cx: &App) -> Img {
    let scale = window.scale_factor();
    let physical_px = (f32::from(size) * scale).round().max(16.0) as u32;
    let svg_renderer = cx.svg_renderer();

    if let Some(render_img) = get_or_render_brand_logo(physical_px, &svg_renderer) {
        img(ImageSource::Render(render_img)).size(size)
    } else {
        // Fallback to static embedded asset
        img("app-icon-256.png").size(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_brand_logo_png_sizes() {
        for &size in &[16, 24, 32, 48, 64, 128, 256] {
            let png = render_brand_logo_png(size);
            assert!(png.is_some(), "Failed to rasterize logo at size {size}");
            let data = png.unwrap();
            assert!(!data.is_empty(), "PNG data is empty for size {size}");
            // PNG signature check
            assert_eq!(&data[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        }
    }
}
