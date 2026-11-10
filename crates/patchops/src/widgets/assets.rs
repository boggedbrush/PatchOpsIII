//! Embedded assets (icons + fonts) so the app has no runtime file or system-font dependency.
//!
//! Fonts are subsets (Basic Latin, Latin-1, common punctuation) of Inter (Regular/Medium/Bold)
//! and JetBrains Mono Regular, both SIL OFL; ~173 KB total.

use std::borrow::Cow;

use gpui::{App, AssetSource, SharedString};

use super::icon::ICON_BYTES;

static FONTS: &[&[u8]] = &[
    include_bytes!("../../assets/fonts/Inter-Regular.otf"),
    include_bytes!("../../assets/fonts/Inter-Medium.otf"),
    include_bytes!("../../assets/fonts/Inter-Bold.otf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
];

/// `Application::new().with_assets(Assets)`.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICON_BYTES
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, b)| Cow::Borrowed(*b)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ICON_BYTES
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}

/// Register the bundled fonts with the text system.
pub fn register_fonts(cx: &mut App) {
    let fonts = FONTS.iter().map(|b| Cow::Borrowed(*b)).collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("patchops: failed to register fonts: {e}");
    }
}
