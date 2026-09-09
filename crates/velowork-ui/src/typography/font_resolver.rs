//! FontResolver implementation for resolving domain fonts.

use super::system_fonts::{detect_system_mono_font, detect_system_ui_font};
use super::types::{FontDomain, ResolvedFont, Typography};
use gpui::px;

/// Configuration options passed into FontResolver.
#[derive(Debug, Clone, Default)]
pub struct FontResolverOptions {
    pub ui_font_family: Option<String>,
    pub ui_font_size: Option<f32>,
    pub mono_font_family: Option<String>,
    pub mono_font_size: Option<f32>,
    pub markdown_font_family: Option<String>,
    pub markdown_font_size: Option<f32>,
    pub terminal_font_family: Option<String>,
    pub terminal_font_size: Option<f32>,
}

pub struct FontResolver;

impl FontResolver {
    /// Resolve font specification for a specific domain.
    pub fn resolve(domain: FontDomain, opts: &FontResolverOptions) -> ResolvedFont {
        let system_ui = detect_system_ui_font();
        let default_ui_size = system_ui.size.unwrap_or(13.0);

        match domain {
            FontDomain::Ui => {
                let family = match &opts.ui_font_family {
                    Some(fam) if !fam.is_empty() && fam != "System Default" && fam != "System" => {
                        fam.clone()
                    }
                    _ => system_ui.family,
                };
                let size = opts.ui_font_size.unwrap_or(default_ui_size);
                ResolvedFont::new(family, px(size))
            }
            FontDomain::Monospace => {
                let family = match &opts.mono_font_family {
                    Some(fam) if !fam.is_empty() && fam != "System Default" && fam != "Auto" => {
                        fam.clone()
                    }
                    _ => detect_system_mono_font(),
                };
                let size = opts.mono_font_size.unwrap_or(default_ui_size);
                ResolvedFont::new(family, px(size))
            }
            FontDomain::Markdown => {
                let ui_font = Self::resolve(FontDomain::Ui, opts);
                let family = match &opts.markdown_font_family {
                    Some(fam) if !fam.is_empty() && fam != "System Default" && fam != "Auto" => {
                        fam.clone()
                    }
                    _ => ui_font.family.to_string(),
                };
                let size = opts.markdown_font_size.unwrap_or(f32::from(ui_font.size));
                ResolvedFont::new(family, px(size))
            }
            FontDomain::Terminal => {
                let family = match &opts.terminal_font_family {
                    Some(fam) if !fam.is_empty() => fam.clone(),
                    _ => "JetBrains Mono".to_string(),
                };
                let size = opts.terminal_font_size.unwrap_or(14.0);
                ResolvedFont::new(family, px(size))
            }
        }
    }

    /// Resolve complete Typography spec for all 4 domains.
    pub fn resolve_all(opts: &FontResolverOptions) -> Typography {
        let ui = Self::resolve(FontDomain::Ui, opts);
        let mono = Self::resolve(FontDomain::Monospace, opts);
        let markdown = Self::resolve(FontDomain::Markdown, opts);
        let terminal = Self::resolve(FontDomain::Terminal, opts);

        Typography {
            ui_font: ui.family,
            ui_size: ui.size,
            mono_font: mono.family,
            mono_size: mono.size,
            markdown_font: markdown.family,
            markdown_size: markdown.size,
            terminal_font: terminal.family,
            terminal_size: terminal.size,
        }
    }
}
