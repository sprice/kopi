use gpui::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;
use std::borrow::Cow;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/*.svg"]
pub struct LocalAssets;

pub struct KopiAssets;

impl AssetSource for KopiAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }

        let local_path = path.strip_prefix("assets/").unwrap_or(path);
        if let Some(file) = LocalAssets::get(local_path) {
            return Ok(Some(file.data));
        }

        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut results: Vec<SharedString> = LocalAssets::iter()
            .filter_map(|p| {
                let full_path = format!("assets/{}", p);
                full_path.starts_with(path).then(|| full_path.into())
            })
            .collect();

        if let Ok(component_assets) = gpui_component_assets::Assets.list(path) {
            results.extend(component_assets);
        }

        Ok(results)
    }
}
