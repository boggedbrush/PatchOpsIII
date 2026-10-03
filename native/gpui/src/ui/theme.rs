//! Design tokens ported from `src/renderer/styles/app.css`.
//!
//! The Electron app computes most sizes with `clamp()`; the values here are the
//! ones that CSS resolves to at the default 1150x820 window.
use gpui::{App, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, Hsla, Pixels, px, rgb};
use gpui_component::Theme;

pub const FONT_XS: f32 = 12.;
pub const FONT_SM: f32 = 13.;
pub const FONT_MD: f32 = 14.;
pub const FONT_LG: f32 = 16.;

pub const CONTROL_HEIGHT: f32 = 36.;
pub const ROW_HEIGHT: f32 = 38.;
pub const NAV_WIDTH: f32 = 190.;
pub const APP_PAD: f32 = 14.;
pub const APP_GAP: f32 = 10.;
pub const PANEL_GAP: f32 = 12.;

/// `--radius`.
pub fn radius() -> Pixels {
    px(9.)
}

/// `calc(var(--radius) - 3px)`: buttons and pills.
pub fn radius_control() -> Pixels {
    px(6.)
}

/// `calc(var(--radius) - 4px)`: inputs, cards and chips.
pub fn radius_card() -> Pixels {
    px(5.)
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

fn tint(value: u32, alpha: f32) -> Hsla {
    hex(value).alpha(alpha)
}

pub fn white(alpha: f32) -> Hsla {
    tint(0xffffff, alpha)
}

pub fn bg() -> Hsla {
    hex(0x0d0d0f)
}

pub fn bg_deep() -> Hsla {
    hex(0x050505)
}

pub fn panel() -> Hsla {
    white(0.055)
}

pub fn panel_strong() -> Hsla {
    white(0.08)
}

pub fn field() -> Hsla {
    tint(0x000000, 0.26)
}

pub fn border() -> Hsla {
    white(0.12)
}

pub fn border_strong() -> Hsla {
    white(0.18)
}

pub fn text() -> Hsla {
    hex(0xf6f6f6)
}

pub fn muted() -> Hsla {
    tint(0xf6f6f6, 0.68)
}

pub fn muted_strong() -> Hsla {
    tint(0xf6f6f6, 0.82)
}

pub fn accent() -> Hsla {
    hex(0xfd0100)
}

pub fn accent_dark() -> Hsla {
    hex(0x9a0f0e)
}

pub fn accent_alpha(alpha: f32) -> Hsla {
    tint(0xfd0100, alpha)
}

pub fn ok() -> Hsla {
    hex(0x34c759)
}

pub fn ok_alpha(alpha: f32) -> Hsla {
    tint(0x34c759, alpha)
}

pub fn warning() -> Hsla {
    hex(0xff9f0a)
}

/// The `#ffb020` amber used by validation and requirement warnings.
pub fn caution() -> Hsla {
    hex(0xffb020)
}

pub fn danger() -> Hsla {
    hex(0xff453a)
}

pub fn danger_alpha(alpha: f32) -> Hsla {
    tint(0xff453a, alpha)
}

/// `#ffd8d4`: text on danger-tinted surfaces.
pub fn danger_text() -> Hsla {
    hex(0xffd8d4)
}

/// Status dot for "not configured" rows.
pub fn dot_idle() -> Hsla {
    hex(0xa8b2be)
}

pub fn modal_surface() -> Hsla {
    hex(0x101011)
}

pub fn scrim() -> Hsla {
    tint(0x000000, 0.62)
}

/// The Electron app's `ui-sans-serif, system-ui, ...` stack. GPUI's
/// `.SystemUIFont` resolves to IBM Plex Sans on Linux, which most systems lack.
pub fn ui_font() -> Font {
    let (family, fallbacks): (&str, &[&str]) = if cfg!(target_os = "macos") {
        (".SystemUIFont", &[])
    } else if cfg!(target_os = "windows") {
        ("Segoe UI", &["Arial"])
    } else {
        (
            "Noto Sans",
            &["Roboto", "DejaVu Sans", "Liberation Sans", "Arial"],
        )
    };
    Font {
        family: family.into(),
        features: FontFeatures::default(),
        fallbacks: Some(FontFallbacks::from_fonts(
            fallbacks.iter().map(|name| (*name).to_owned()).collect(),
        )),
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    }
}

/// Mirror the CSS variables onto gpui-component's theme so its widgets
/// (inputs, sliders, scrollbars) match the hand-built controls.
pub fn apply(cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.font_family = ui_font().family;
    theme.font_size = px(FONT_MD);
    theme.mono_font_size = px(FONT_XS);
    theme.radius = radius_card();
    theme.radius_lg = radius();
    theme.shadow = false;
    let colors = &mut theme.colors;
    colors.background = bg();
    colors.foreground = text();
    colors.border = border();
    colors.input = border();
    colors.caret = text();
    colors.selection = accent_alpha(0.32);
    colors.muted = panel_strong();
    colors.muted_foreground = muted();
    colors.accent = white(0.07);
    colors.accent_foreground = text();
    colors.primary = accent();
    colors.primary_hover = hex(0xf03535);
    colors.primary_active = accent_dark();
    colors.primary_foreground = hex(0xffffff);
    colors.secondary = panel_strong();
    colors.secondary_hover = white(0.12);
    colors.secondary_active = white(0.16);
    colors.secondary_foreground = text();
    colors.ring = accent_alpha(0.72);
    colors.danger = danger();
    colors.success = ok();
    colors.warning = warning();
    colors.popover = modal_surface();
    colors.popover_foreground = text();
    colors.slider_bar = accent();
    colors.slider_thumb = text();
    colors.scrollbar = white(0.045);
    colors.scrollbar_thumb = white(0.26);
    colors.scrollbar_thumb_hover = accent_alpha(0.58);
    colors.switch = field();
    colors.switch_thumb = muted();
}
