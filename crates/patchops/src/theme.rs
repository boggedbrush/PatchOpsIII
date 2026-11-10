//! Colours and metrics. Registered as a GPUI global; read with `cx.theme()`.

use gpui::{rgb, App, Global, Hsla};

#[derive(Clone)]
pub struct Theme {
    pub bg: Hsla,
    pub sidebar: Hsla,
    pub panel: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub hover: Hsla,
    pub selection: Hsla,
    pub danger: Hsla,
    pub font_ui: &'static str,
    pub font_mono: &'static str,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            bg: c(0x14161a),
            sidebar: c(0x1b1e24),
            panel: c(0x22262e),
            border: c(0x343944),
            text: c(0xe6e8ec),
            text_muted: c(0x8b93a1),
            accent: c(0x3b82f6),
            accent_hover: c(0x5b9bf8),
            hover: c(0x2c313b),
            selection: c(0x3b82f6).opacity(0.35),
            danger: c(0xef4444),
            font_ui: "Inter",
            font_mono: "DejaVu Sans Mono",
        }
    }
}

impl Global for Theme {}

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}
