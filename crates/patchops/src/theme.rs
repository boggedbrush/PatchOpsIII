//! Design tokens ported from the legacy `src/renderer/styles/app.css` (dark theme, red accent).
//! Registered as a GPUI global; read with `cx.theme()`.
//!
//! Colours are `Hsla` (many are white/black with alpha, exactly like the CSS `rgba(...)`
//! tokens, so they composite over whatever surface they sit on). Metrics are plain `f32`
//! pixel values; wrap with `px()` at the call site.

use gpui::{hsla, rgb, App, Global, Hsla};

#[derive(Clone, Copy)]
pub struct Theme {
    // surfaces
    pub bg: Hsla,
    pub bg_deep: Hsla,
    /// Nav / side panel surface (`rgba(255,255,255,.045)`).
    pub sidebar: Hsla,
    /// Card / panel body surface (`--panel`).
    pub panel: Hsla,
    /// Raised surface, buttons (`--panel-strong`).
    pub panel_strong: Hsla,
    /// Input background (`--field`).
    pub field: Hsla,
    /// Modal surface.
    pub modal: Hsla,
    pub overlay: Hsla,
    // lines
    pub border: Hsla,
    pub border_strong: Hsla,
    // text
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_strong: Hsla,
    // accent
    pub accent: Hsla,
    pub accent_dark: Hsla,
    pub accent_hover: Hsla,
    pub accent_bg: Hsla,
    pub accent_border: Hsla,
    // state
    pub ok: Hsla,
    pub ok_bg: Hsla,
    pub ok_border: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub danger_bg: Hsla,
    pub danger_border: Hsla,
    // interaction
    pub hover: Hsla,
    pub hover_strong: Hsla,
    pub selection: Hsla,
    pub focus_ring: Hsla,
    pub scroll_track: Hsla,
    pub scroll_thumb: Hsla,
    pub scroll_thumb_hover: Hsla,
    // metrics (px)
    pub radius: f32,
    pub radius_sm: f32,
    pub radius_xs: f32,
    pub control_h: f32,
    pub row_h: f32,
    pub nav_w: f32,
    pub font_xs: f32,
    pub font_sm: f32,
    pub font_md: f32,
    pub font_lg: f32,
    pub font_ui: &'static str,
    pub font_mono: &'static str,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn white(a: f32) -> Hsla {
    hsla(0., 0., 1., a)
}

fn black(a: f32) -> Hsla {
    hsla(0., 0., 0., a)
}

impl Theme {
    pub fn dark() -> Self {
        let accent = c(0xfd0100);
        let ok = c(0x34c759);
        let danger = c(0xff453a);
        Self {
            bg: c(0x0d0d0f),
            bg_deep: c(0x050505),
            sidebar: white(0.045),
            panel: white(0.055),
            panel_strong: white(0.08),
            field: black(0.26),
            modal: c(0x101011),
            overlay: black(0.62),
            border: white(0.12),
            border_strong: white(0.18),
            text: c(0xf6f6f6),
            text_muted: hsla(0., 0., 0.9647, 0.68),
            text_strong: hsla(0., 0., 0.9647, 0.82),
            accent,
            accent_dark: c(0x9a0f0e),
            accent_hover: c(0xff3a34),
            accent_bg: accent.opacity(0.16),
            accent_border: accent.opacity(0.28),
            ok,
            ok_bg: ok.opacity(0.16),
            ok_border: ok.opacity(0.38),
            warning: c(0xff9f0a),
            danger,
            danger_bg: danger.opacity(0.12),
            danger_border: danger.opacity(0.38),
            hover: white(0.06),
            hover_strong: white(0.12),
            selection: accent.opacity(0.40),
            focus_ring: accent.opacity(0.72),
            scroll_track: white(0.045),
            scroll_thumb: white(0.26),
            scroll_thumb_hover: accent.opacity(0.58),
            radius: 10.0,
            radius_sm: 7.0,
            radius_xs: 5.0,
            control_h: 36.0,
            row_h: 38.0,
            nav_w: 190.0,
            font_xs: 12.0,
            font_sm: 13.0,
            font_md: 14.0,
            font_lg: 16.0,
            font_ui: "Inter",
            font_mono: "JetBrains Mono",
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
