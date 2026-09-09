use crate::terminal_view_settings;
use alacritty_terminal::grid::Dimensions;
use gpui::*;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use velowork_ui::theme::theme;
use velowork_terminal::terminal::{Terminal, TerminalSize};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::theme::{bg_opacity, with_alpha};
use velowork_workspace::settings::CursorShape;

use super::terminal_input::TerminalInputHandler;
use crate::rendering::{TerminalPaintOptions, TerminalPainter, TerminalRenderGeometry, TerminalRenderModel};

type ResizeViewerSizes = HashMap<String, HashMap<u64, TerminalSize>>;

static NEXT_RESIZE_VIEWER_ID: AtomicU64 = AtomicU64::new(1);
static RESIZE_VIEWER_SIZES: OnceLock<Mutex<ResizeViewerSizes>> = OnceLock::new();

pub(crate) fn next_resize_viewer_id() -> u64 {
    NEXT_RESIZE_VIEWER_ID.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::{deregister_resize_viewer, shared_resize_target};
    use velowork_terminal::terminal::TerminalSize;

    fn size(cols: u16, rows: u16) -> TerminalSize {
        TerminalSize {
            cols,
            rows,
            cell_width: 8.0,
            cell_height: 16.0,
        }
    }

    #[test]
    fn shared_resize_target_uses_per_dimension_minimum() {
        let terminal_id = "shared_resize_target_uses_per_dimension_minimum";

        let (count, target) = shared_resize_target(terminal_id, 1, size(120, 15));
        assert_eq!(count, 1);
        assert_eq!((target.cols, target.rows), (120, 15));

        let (count, target) = shared_resize_target(terminal_id, 2, size(80, 40));
        assert_eq!(count, 2);
        assert_eq!((target.cols, target.rows), (80, 15));

        deregister_resize_viewer(terminal_id, 1);
        deregister_resize_viewer(terminal_id, 2);
    }

    #[test]
    fn shared_resize_target_grows_when_every_viewer_can_fit() {
        let terminal_id = "shared_resize_target_grows_when_every_viewer_can_fit";

        let _ = shared_resize_target(terminal_id, 1, size(80, 15));
        let _ = shared_resize_target(terminal_id, 2, size(80, 20));
        let (count, target) = shared_resize_target(terminal_id, 1, size(100, 25));

        assert_eq!(count, 2);
        assert_eq!((target.cols, target.rows), (80, 20));

        deregister_resize_viewer(terminal_id, 1);
        deregister_resize_viewer(terminal_id, 2);
    }

    #[test]
    fn deregistered_viewer_no_longer_clamps_resize_target() {
        let terminal_id = "deregistered_viewer_no_longer_clamps_resize_target";

        let _ = shared_resize_target(terminal_id, 1, size(80, 15));
        deregister_resize_viewer(terminal_id, 1);
        let (count, target) = shared_resize_target(terminal_id, 2, size(120, 40));

        assert_eq!(count, 1);
        assert_eq!((target.cols, target.rows), (120, 40));

        deregister_resize_viewer(terminal_id, 2);
    }
}

pub(crate) fn deregister_resize_viewer(terminal_id: &str, viewer_id: u64) {
    let mut sizes = resize_viewer_sizes().lock();
    if let Some(viewers) = sizes.get_mut(terminal_id) {
        viewers.remove(&viewer_id);
        if viewers.is_empty() {
            sizes.remove(terminal_id);
        }
    }
}

fn resize_viewer_sizes() -> &'static Mutex<ResizeViewerSizes> {
    RESIZE_VIEWER_SIZES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn shared_resize_target(
    terminal_id: &str,
    viewer_id: u64,
    desired_size: TerminalSize,
) -> (usize, TerminalSize) {
    let mut sizes = resize_viewer_sizes().lock();
    let viewers = sizes.entry(terminal_id.to_string()).or_default();
    viewers.insert(viewer_id, desired_size);

    let viewer_count = viewers.len();
    let min_cols = viewers
        .values()
        .map(|size| size.cols)
        .min()
        .unwrap_or(desired_size.cols);
    let min_rows = viewers
        .values()
        .map(|size| size.rows)
        .min()
        .unwrap_or(desired_size.rows);

    (
        viewer_count,
        TerminalSize {
            cols: min_cols,
            rows: min_rows,
            cell_width: desired_size.cell_width,
            cell_height: desired_size.cell_height,
        },
    )
}

/// A search match in the terminal grid
#[derive(Clone, Debug)]
pub struct SearchMatch {
    pub line: i32,
    pub col: usize,
    pub len: usize,
}

/// The kind of link detected in the terminal
#[derive(Clone, Debug, PartialEq)]
pub enum LinkKind {
    /// A web URL (http/https)
    Url,
    /// A file path, optionally with line and column numbers
    FilePath { line: Option<u32>, col: Option<u32> },
}

/// A detected URL or file path in the terminal grid
#[derive(Clone, Debug)]
pub struct URLMatch {
    pub line: i32,
    pub col: usize,
    pub len: usize,
    pub url: String,
    pub kind: LinkKind,
    /// Group ID: segments of the same wrapped URL share the same group
    pub link_group: usize,
}

/// Custom GPUI element for rendering a terminal
pub struct TerminalElement {
    terminal: Arc<Terminal>,
    focus_handle: FocusHandle,
    resize_viewer_id: u64,
    search_matches: Arc<Vec<SearchMatch>>,
    current_match_index: Option<usize>,
    url_matches: Arc<Vec<URLMatch>>,
    hovered_url_group: Option<usize>,
    cursor_visible: bool,
    cursor_style: CursorShape,
    zoom_level: f32,
    scroll_x: f32,
    /// Optional background tint color (u32 RGB) blended softly into the terminal background.
    bg_tint: Option<u32>,
    font_family: Option<String>,
    font_size: Option<f32>,
    color_scheme: Option<String>,
    bottom_left_radius: f32,
    bottom_right_radius: f32,
}

impl TerminalElement {
    pub fn new(terminal: Arc<Terminal>, focus_handle: FocusHandle, resize_viewer_id: u64) -> Self {
        Self {
            terminal,
            focus_handle,
            resize_viewer_id,
            search_matches: Arc::new(Vec::new()),
            current_match_index: None,
            url_matches: Arc::new(Vec::new()),
            hovered_url_group: None,
            cursor_visible: true,
            cursor_style: CursorShape::Block,
            zoom_level: 1.0,
            scroll_x: 0.0,
            bg_tint: None,
            font_family: None,
            font_size: None,
            color_scheme: None,
            bottom_left_radius: 0.0,
            bottom_right_radius: 0.0,
        }
    }

    pub fn with_font_family(mut self, font_family: String) -> Self {
        self.font_family = Some(font_family);
        self
    }

    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = Some(font_size);
        self
    }

    pub fn with_color_scheme(mut self, color_scheme: String) -> Self {
        self.color_scheme = Some(color_scheme);
        self
    }

    pub fn with_scroll_x(mut self, scroll_x: f32) -> Self {
        self.scroll_x = scroll_x;
        self
    }

    pub fn with_bg_tint(mut self, tint: Option<u32>) -> Self {
        self.bg_tint = tint;
        self
    }

    pub fn with_zoom(mut self, zoom_level: f32) -> Self {
        self.zoom_level = zoom_level;
        self
    }

    pub fn with_search(
        mut self,
        search_matches: Arc<Vec<SearchMatch>>,
        current_match_index: Option<usize>,
    ) -> Self {
        self.search_matches = search_matches;
        self.current_match_index = current_match_index;
        self
    }

    pub fn with_urls(
        mut self,
        url_matches: Arc<Vec<URLMatch>>,
        hovered_url_group: Option<usize>,
    ) -> Self {
        self.url_matches = url_matches;
        self.hovered_url_group = hovered_url_group;
        self
    }

    pub fn with_cursor_visible(mut self, visible: bool) -> Self {
        self.cursor_visible = visible;
        self
    }

    pub fn with_cursor_style(mut self, style: CursorShape) -> Self {
        self.cursor_style = style;
        self
    }

    pub fn with_bottom_corner_radii(mut self, bl: f32, br: f32) -> Self {
        self.bottom_left_radius = bl;
        self.bottom_right_radius = br;
        self
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// State for terminal element layout
pub struct TerminalElementState {
    cell_width: Pixels,
    line_height: Pixels,
    font_size: Pixels,
    font: Font,
    /// Pre-computed font variants to avoid cloning in hot path
    font_bold: Font,
    font_italic: Font,
    font_bold_italic: Font,
}

impl Element for TerminalElement {
    type RequestLayoutState = TerminalElementState;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // Get font settings from global settings or session override, apply per-terminal zoom and UI/OS scale factor
        let app_settings = terminal_view_settings(cx);
        let base_font_size = self.font_size.unwrap_or(app_settings.font_size);
        let font_size = px(base_font_size * self.zoom_level * velowork_ui::tokens::ui_scale_factor(cx));
        let line_height_multiplier = app_settings.line_height;
        let font_family = self.font_family.clone().unwrap_or_else(|| app_settings.font_family.clone());

        // Resolve the configured terminal base weight. The bold/italic
        // ANSI variants (below) override these, so this only affects regular text.
        let base_weight = match app_settings.font_weight.as_str() {
            "Light" => FontWeight::LIGHT,
            "Medium" => FontWeight::MEDIUM,
            "Bold" => FontWeight::BOLD,
            _ => FontWeight::NORMAL,
        };
        let base_style = FontStyle::Normal;

        // Use configured font family with fallbacks
        #[cfg(target_os = "macos")]
        let font = Font {
            family: font_family.into(),
            features: FontFeatures::disable_ligatures(),
            fallbacks: Some(FontFallbacks::from_fonts(vec![
                "JetBrains Mono".into(),
                "Menlo".into(),
                "SF Mono".into(),
                "Monaco".into(),
            ])),
            weight: base_weight,
            style: base_style,
        };

        #[cfg(not(target_os = "macos"))]
        let font = Font {
            family: font_family.into(),
            features: FontFeatures::disable_ligatures(),
            fallbacks: Some(FontFallbacks::from_fonts(vec![
                "JetBrains Mono".into(),
                "DejaVu Sans Mono".into(),
                "Liberation Mono".into(),
                "Ubuntu Mono".into(),
                "Noto Sans Mono".into(),
                "monospace".into(),
            ])),
            weight: base_weight,
            style: base_style,
        };

        // Pre-compute font variants to avoid cloning in hot path
        let font_bold = Font {
            weight: FontWeight::BOLD,
            ..font.clone()
        };
        let font_italic = Font {
            style: FontStyle::Italic,
            ..font.clone()
        };
        let font_bold_italic = Font {
            weight: FontWeight::BOLD,
            style: FontStyle::Italic,
            ..font.clone()
        };

        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&font);

        // Use advance() for proper cell width (like Zed)
        let cell_width = text_system
            .advance(font_id, font_size, 'm')
            .map(|size| size.width)
            .unwrap_or(font_size * 0.6);

        // Line height from settings
        let line_height = font_size * line_height_multiplier;

        let style = Style {
            size: Size {
                width: relative(1.0).into(),
                height: relative(1.0).into(),
            },
            ..Default::default()
        };

        let layout_id = window.request_layout(style, [], cx);

        (
            layout_id,
            TerminalElementState {
                cell_width,
                line_height,
                font_size,
                font,
                font_bold,
                font_italic,
                font_bold_italic,
            },
        )
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _state: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Get theme colors
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        // Terminal transparency: the content default background follows the global
        // background opacity. When a background image is configured, the default
        // background is rendered fully transparent so the image shows through.
        let tvs = terminal_view_settings(cx);
        let image_set = tvs
            .terminal_background_image
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let base_alpha = if image_set { 0.0 } else { bg_opacity(cx) };

        let cell_width = state.cell_width;
        let line_height = state.line_height;
        let font_size = state.font_size;
        let cell_width_f = f32::from(cell_width);
        let line_height_f = f32::from(line_height);

        // Calculate cursor position for IME candidate window placement
        let cursor_bounds = self.terminal.with_content(|term| {
            let cursor_point = term.grid().cursor.point;
            let display_offset = term.grid().display_offset() as i32;
            let cursor_visual_line = cursor_point.line.0 + display_offset;
            if cursor_visual_line >= 0 {
                let cursor_x = px((f32::from(bounds.origin.x)
                    + cursor_point.column.0 as f32 * cell_width_f)
                    .floor());
                let cursor_y = px((f32::from(bounds.origin.y)
                    + cursor_visual_line as f32 * line_height_f)
                    .floor());
                Some(Bounds {
                    origin: point(cursor_x, cursor_y),
                    size: size(cell_width, line_height),
                })
            } else {
                None
            }
        });

        // Register input handler
        let input_handler = TerminalInputHandler {
            terminal: self.terminal.clone(),
            cursor_bounds,
        };
        window.handle_input(&self.focus_handle, input_handler, cx);

        // Calculate terminal size and resize if needed
        let available_width = f32::from(bounds.size.width);
        let available_height = f32::from(bounds.size.height);

        let visible_cols = ((available_width - 0.5) / cell_width_f).floor().max(1.0) as u16;
        let new_rows = ((available_height - 0.5) / line_height_f).floor().max(1.0) as u16;
        // PTY 列数必须始终与窗口可视列数 (visible_cols) 保持一致，
        // 使得 zsh / bash / RPROMPT 等 shell 交互进程能正确感知真实屏幕宽度，
        // 彻底防止在 NoWrap 模式下 zsh prompt 收到虚假 1000 列信号导致无限向右延伸与错位。
        let new_cols = visible_cols;

        let desired_size = TerminalSize {
            cols: new_cols,
            rows: new_rows,
            cell_width: cell_width_f,
            cell_height: line_height_f,
        };
        let (n_viewers, resize_size) = shared_resize_target(
            &self.terminal.terminal_id,
            self.resize_viewer_id,
            desired_size,
        );

        let current_size = self.terminal.resize_state.lock().size;
        let cols_rows_changed =
            resize_size.cols != current_size.cols || resize_size.rows != current_size.rows;
        let cell_size_changed = (cell_width_f - current_size.cell_width).abs() > 0.001
            || (line_height_f - current_size.cell_height).abs() > 0.001;

        if cols_rows_changed && self.terminal.is_resize_owner_local() {
            let target = if n_viewers <= 1 {
                desired_size
            } else {
                resize_size
            };
            self.terminal.resize(target);
        } else if cell_size_changed {
            let mut rs = self.terminal.resize_state.lock();
            rs.size.cell_width = cell_width_f;
            rs.size.cell_height = line_height_f;
        }

        let active_color_scheme = self.color_scheme.as_deref().unwrap_or(&tvs.color_scheme);
        let term_palette = velowork_core::theme::get_terminal_palette_with_custom(
            active_color_scheme,
            &tvs.custom_terminal_color_schemes,
        );
        self.terminal.push_terminal_palette(term_palette);

        let selection = self.terminal.selection_bounds();

        let cursor_visible = self.cursor_visible;
        let cursor_style = match self.terminal.app_cursor_shape() {
            Some(velowork_terminal::terminal::AppCursorShape::Block) => CursorShape::Block,
            Some(velowork_terminal::terminal::AppCursorShape::Bar) => CursorShape::Bar,
            Some(velowork_terminal::terminal::AppCursorShape::Underline) => CursorShape::Underline,
            None => self.cursor_style,
        };

        let scroll_x_offset = if tvs.wrap_mode == velowork_workspace::settings::WrapMode::NoWrap {
            self.scroll_x
        } else {
            0.0
        };

        let origin_x = f32::from(bounds.origin.x) - scroll_x_offset;

        let geometry = TerminalRenderGeometry::new(
            Bounds {
                origin: point(px(origin_x), bounds.origin.y),
                size: bounds.size,
            },
            cell_width,
            line_height,
            font_size,
        );

        let options = TerminalPaintOptions::full(cursor_visible);

        self.terminal.with_content(|term| {
            let grid = term.grid();
            let screen_lines = grid.screen_lines();
            let display_offset = grid.display_offset() as i32;

            let model = TerminalRenderModel {
                grid,
                cols: grid.columns(),
                screen_lines,
                display_offset,
                cursor_point: Some(term.grid().cursor.point),
                cursor_shape: cursor_style,
                cursor_visible,
                selection,
                search_matches: &self.search_matches,
                current_match_index: self.current_match_index,
                url_matches: &self.url_matches,
                hovered_url_group: self.hovered_url_group,
            };

            let painter = TerminalPainter {
                model: &model,
                geometry: &geometry,
                options: &options,
                palette: &term_palette,
                theme_colors: &t,
                font: state.font.clone(),
                font_bold: state.font_bold.clone(),
                font_italic: state.font_italic.clone(),
                font_bold_italic: state.font_bold_italic.clone(),
            };

            painter.paint(window, cx);

            // Paint IME composition (marked text) if active
            if let Some(marked) = self.terminal.marked_text() {
                let cursor_point = term.grid().cursor.point;
                let cursor_visual_line = cursor_point.line.0 + display_offset;
                if cursor_visual_line >= 0 && cursor_visual_line < screen_lines as i32 {
                    let marked_len = marked.chars().count();
                    let start_x = px((f32::from(origin_x)
                        + cursor_point.column.0 as f32 * cell_width_f)
                        .floor());
                    let start_y = px((f32::from(bounds.origin.y)
                        + cursor_visual_line as f32 * line_height_f)
                        .floor());

                    let bg_color = with_alpha(term_palette.background, base_alpha);
                    let bg_bounds = Bounds {
                        origin: point(start_x, start_y),
                        size: size(px(cell_width_f * marked_len as f32), line_height),
                    };
                    window.paint_quad(fill(bg_bounds, bg_color));

                    let run_style = TextRun {
                        len: marked.len(),
                        font: state.font.clone(),
                        color: p.text_primary.into(),
                        background_color: None,
                        underline: Some(UnderlineStyle {
                            color: Some(p.text_primary.into()),
                            thickness: px(1.0),
                            wavy: false,
                        }),
                        strikethrough: None,
                    };

                    let _ = window
                        .text_system()
                        .shape_line(marked.into(), font_size, &[run_style], Some(cell_width))
                        .paint(
                            point(start_x, start_y),
                            line_height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                }
            }
        });

        // Phase 5: (removed) The unfocused "fog" overlay that previously
        // dimmed terminals on focus loss has been deleted. It behaved like a
        // mask whose semi-transparent quad lagged the content texture during
        // resize, exposing a visible seam. Focus is now signalled by the pane
        // border / cursor blink / tab indicator instead.
    }
}
