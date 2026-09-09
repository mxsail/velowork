use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets"]
#[exclude = "*.DS_Store"]
pub struct ProjectAssets;

/// Get embedded fonts for registration with GPUI
#[allow(
    clippy::expect_used,
    reason = "fonts are embedded via RustEmbed at compile time — missing asset means the build is broken"
)]
pub fn embedded_fonts() -> Vec<Cow<'static, [u8]>> {
    vec![
        ProjectAssets::get("fonts/JetBrainsMono-Regular.ttf")
            .expect("JetBrainsMono-Regular.ttf not found")
            .data,
        ProjectAssets::get("fonts/JetBrainsMono-Bold.ttf")
            .expect("JetBrainsMono-Bold.ttf not found")
            .data,
    ]
}


pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }

        let clean_path = path.trim_start_matches('/');
        if let Some(asset) = ProjectAssets::get(clean_path) {
            return Ok(Some(asset.data));
        }

        Ok(None)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let clean_path = path.trim_start_matches('/');
        let results: Vec<SharedString> = ProjectAssets::iter()
            .filter(|p| p.starts_with(clean_path))
            .map(SharedString::from)
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_assets_loading() {
        let assets = Assets;

        assert!(assets.load("logo.png").unwrap().is_some());
        assert!(assets.load("app-icon.ico").unwrap().is_some());
        assert!(assets.load("velowork_icon.svg").unwrap().is_some());
        assert!(assets.load("app-icon-256.png").unwrap().is_some());

        assert!(assets.load("icons/terminal.svg").unwrap().is_some());
        assert!(assets.load("/icons/terminal.svg").unwrap().is_some());
        assert!(assets.load("fonts/JetBrainsMono-Regular.ttf").unwrap().is_some());

        for icon in velowork_ui::icon::AppIcon::all() {
            let path = icon.path();
            assert!(
                assets.load(path).unwrap().is_some(),
                "Failed to load asset for icon: {path}"
            );
            assert!(
                assets.load(&format!("/{path}")).unwrap().is_some(),
                "Failed to load leading slash asset for icon: /{path}"
            );
        }
    }
}
