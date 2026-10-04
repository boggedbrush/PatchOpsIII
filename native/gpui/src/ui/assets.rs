//! Embedded assets: Lucide icons (the same set the Electron UI uses) and the app logo.
use anyhow::Result;
use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

const ASSETS: &[(&str, &[u8])] = &[
    (
        "caption/close.svg",
        include_bytes!("../../assets/caption/close.svg"),
    ),
    (
        "caption/maximize.svg",
        include_bytes!("../../assets/caption/maximize.svg"),
    ),
    (
        "caption/minimize.svg",
        include_bytes!("../../assets/caption/minimize.svg"),
    ),
    (
        "caption/restore.svg",
        include_bytes!("../../assets/caption/restore.svg"),
    ),
    (
        "icons/check.svg",
        include_bytes!("../../assets/icons/check.svg"),
    ),
    (
        "icons/chevron-down.svg",
        include_bytes!("../../assets/icons/chevron-down.svg"),
    ),
    (
        "icons/chevron-right.svg",
        include_bytes!("../../assets/icons/chevron-right.svg"),
    ),
    (
        "icons/chevron-up.svg",
        include_bytes!("../../assets/icons/chevron-up.svg"),
    ),
    (
        "icons/circle-check.svg",
        include_bytes!("../../assets/icons/circle-check.svg"),
    ),
    (
        "icons/clipboard.svg",
        include_bytes!("../../assets/icons/clipboard.svg"),
    ),
    (
        "icons/close.svg",
        include_bytes!("../../assets/icons/close.svg"),
    ),
    (
        "icons/cpu.svg",
        include_bytes!("../../assets/icons/cpu.svg"),
    ),
    (
        "icons/download.svg",
        include_bytes!("../../assets/icons/download.svg"),
    ),
    (
        "icons/eraser.svg",
        include_bytes!("../../assets/icons/eraser.svg"),
    ),
    (
        "icons/external-link.svg",
        include_bytes!("../../assets/icons/external-link.svg"),
    ),
    (
        "icons/eye.svg",
        include_bytes!("../../assets/icons/eye.svg"),
    ),
    (
        "icons/eye-off.svg",
        include_bytes!("../../assets/icons/eye-off.svg"),
    ),
    (
        "icons/folder-open.svg",
        include_bytes!("../../assets/icons/folder-open.svg"),
    ),
    (
        "icons/gem.svg",
        include_bytes!("../../assets/icons/gem.svg"),
    ),
    (
        "icons/image.svg",
        include_bytes!("../../assets/icons/image.svg"),
    ),
    (
        "icons/key-round.svg",
        include_bytes!("../../assets/icons/key-round.svg"),
    ),
    (
        "icons/layout-dashboard.svg",
        include_bytes!("../../assets/icons/layout-dashboard.svg"),
    ),
    (
        "icons/refresh-cw.svg",
        include_bytes!("../../assets/icons/refresh-cw.svg"),
    ),
    (
        "icons/rotate-ccw.svg",
        include_bytes!("../../assets/icons/rotate-ccw.svg"),
    ),
    (
        "icons/save.svg",
        include_bytes!("../../assets/icons/save.svg"),
    ),
    (
        "icons/shield-check.svg",
        include_bytes!("../../assets/icons/shield-check.svg"),
    ),
    (
        "icons/square-play.svg",
        include_bytes!("../../assets/icons/square-play.svg"),
    ),
    (
        "icons/trash-2.svg",
        include_bytes!("../../assets/icons/trash-2.svg"),
    ),
    (
        "icons/triangle-alert.svg",
        include_bytes!("../../assets/icons/triangle-alert.svg"),
    ),
    (
        "icons/wrench.svg",
        include_bytes!("../../assets/icons/wrench.svg"),
    ),
    ("icons/x.svg", include_bytes!("../../assets/icons/x.svg")),
    (
        "images/logo.png",
        include_bytes!("../../assets/images/logo.png"),
    ),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ASSETS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ASSETS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
