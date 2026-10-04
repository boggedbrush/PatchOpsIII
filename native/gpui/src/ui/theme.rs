//! Design tokens ported from `src/renderer/styles/app.css`.
//!
//! The Electron app computes most sizes with `clamp()`; the values here are the
//! ones that CSS resolves to at the default 1150x820 window.
use gpui::{App, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, Hsla, Pixels, px, rgb};
use gpui_component::Theme;
use std::sync::atomic::{AtomicU8, Ordering};

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

/// How much of the desktop shows through the window.
///
/// The shell (page background, titlebar, nav) turns translucent while cards,
/// fields and the activity log keep a near-opaque base so text stays legible
/// (>= 4.5:1 for body and muted text even over a pure-white wallpaper; see
/// the per-surface notes below). `Opaque` reproduces the Electron look exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// Fully opaque surfaces: X11 without a compositor, GNOME, and
    /// `PATCHOPSIII_GPUI_OPAQUE=1`.
    Opaque,
    /// Plain alpha transparency without a blur (X11 compositors, wlroots-style
    /// Wayland compositors that may blur on their own).
    Translucent,
    /// Transparency over a blurred desktop: Windows acrylic, KDE Plasma
    /// (`org_kde_kwin_blur`), macOS.
    Blurred,
}

static BACKDROP: AtomicU8 = AtomicU8::new(0);

pub fn set_backdrop(backdrop: Backdrop) {
    BACKDROP.store(backdrop as u8, Ordering::Relaxed);
}

pub fn backdrop() -> Backdrop {
    match BACKDROP.load(Ordering::Relaxed) {
        1 => Backdrop::Translucent,
        2 => Backdrop::Blurred,
        _ => Backdrop::Opaque,
    }
}

/// Alpha of the page background and the nav column.
fn shell_alpha() -> f32 {
    match backdrop() {
        Backdrop::Opaque => 1.,
        Backdrop::Translucent => 0.92,
        // 0.84 over a pure-white wallpaper still gives muted text 6.4:1 (and
        // body text 12:1), so it stays AA even where no blur is applied.
        Backdrop::Blurred => 0.84,
    }
}

/// Alpha of cards, buttons and input wells. The raised look of the Electron
/// UI comes from a faint white overlay; when the window is translucent the
/// overlay is replaced by its pre-composited colour plus a high alpha, so the
/// surface does not turn into a pane of glass.
fn card_alpha() -> f32 {
    match backdrop() {
        Backdrop::Opaque => 1.,
        Backdrop::Translucent => 0.97,
        // 0.92 over white: body text 12.6:1, muted text 6.8:1.
        Backdrop::Blurred => 0.92,
    }
}

fn is_translucent() -> bool {
    backdrop() != Backdrop::Opaque
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

/// The page background (`--bg`), bottom of the app gradient.
pub fn bg() -> Hsla {
    tint(0x0d0d0f, shell_alpha())
}

/// `#050505`: top of the app gradient.
pub fn bg_deep() -> Hsla {
    tint(0x050505, shell_alpha())
}

/// The titlebar strip, painted over the page gradient. Translucent modes use
/// a black wash instead of a second opaque layer, which would stack with the
/// page and end up fully opaque (page 0.84 + wash 0.35 = 0.90 overall).
pub fn titlebar() -> Hsla {
    match backdrop() {
        Backdrop::Opaque => hex(0x050505),
        Backdrop::Translucent => tint(0x000000, 0.30),
        Backdrop::Blurred => tint(0x000000, 0.35),
    }
}

/// `.panel` fill: `white(0.055)` over the page.
pub fn panel() -> Hsla {
    if is_translucent() {
        tint(0x1a1a1c, card_alpha())
    } else {
        white(0.055)
    }
}

/// Buttons: `white(0.08)` over the page.
pub fn panel_strong() -> Hsla {
    if is_translucent() {
        tint(0x202022, card_alpha())
    } else {
        white(0.08)
    }
}

/// Input wells: `black(0.26)` over a panel.
pub fn field() -> Hsla {
    if is_translucent() {
        tint(0x131314, (card_alpha() + 0.03).min(1.))
    } else {
        tint(0x000000, 0.26)
    }
}

/// The activity log's well (`--bg-deep` at 50% over its panel).
pub fn log_surface() -> Hsla {
    if is_translucent() {
        tint(0x050505, 0.9)
    } else {
        tint(0x050505, 0.5)
    }
}

/// The 1px frame drawn around a client-decorated window.
pub fn window_edge() -> Hsla {
    white(0.16)
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
    // `Root` paints `background` (and a square `window_border`) under the
    // view; the app paints its own rounded, possibly translucent fill, so
    // both must stay clear.
    colors.background = gpui::transparent_black();
    colors.window_border = gpui::transparent_black();
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
