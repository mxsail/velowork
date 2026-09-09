use gpui::*;

/// Terminal rendering geometry defining viewport bounding box and cell dimensions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalRenderGeometry {
    pub bounds: Bounds<Pixels>,
    pub cell_width: Pixels,
    pub line_height: Pixels,
    pub font_size: Pixels,
}

impl TerminalRenderGeometry {
    /// Construct exact geometry from given bounds and cell metrics.
    pub fn new(bounds: Bounds<Pixels>, cell_width: Pixels, line_height: Pixels, font_size: Pixels) -> Self {
        Self {
            bounds,
            cell_width,
            line_height,
            font_size,
        }
    }

    /// Calculate scaled geometry fitting inside `available_bounds` while preserving cell aspect ratio (`Contain`).
    pub fn fit_contain(
        available_bounds: Bounds<Pixels>,
        cols: usize,
        rows: usize,
        base_cell_w: f32,
        base_cell_h: f32,
    ) -> Self {
        let cols = cols.max(1) as f32;
        let rows = rows.max(1) as f32;

        let aspect_ratio = (base_cell_w / base_cell_h.max(1.0)).clamp(0.3, 1.0);
        let avail_w = f32::from(available_bounds.size.width);
        let avail_h = f32::from(available_bounds.size.height);

        // Terminal total aspect ratio = (cols * aspect_ratio) / rows
        let content_aspect = (cols * aspect_ratio) / rows;
        let container_aspect = avail_w / avail_h.max(1.0);

        let (final_w, final_h) = if content_aspect > container_aspect {
            // Limited by width
            let w = avail_w;
            let h = (w / content_aspect).min(avail_h);
            (w, h)
        } else {
            // Limited by height
            let h = avail_h;
            let w = (h * content_aspect).min(avail_w);
            (w, h)
        };

        let origin_x = available_bounds.origin.x + px((avail_w - final_w) * 0.5);
        let origin_y = available_bounds.origin.y + px((avail_h - final_h) * 0.5);

        let bounds = Bounds {
            origin: point(origin_x, origin_y),
            size: size(px(final_w), px(final_h)),
        };

        let cell_width = px(final_w / cols);
        let line_height = px(final_h / rows);
        let font_size = px((final_h / rows) * 0.82);

        Self {
            bounds,
            cell_width,
            line_height,
            font_size,
        }
    }
}
