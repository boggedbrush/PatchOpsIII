//! Status display: `StatusRow`, `StatusPill`, `Metric`, `Badge`, `Alert`, `Chip`, `Tone`.

use gpui::{
    div, prelude::*, px, AnyElement, App, ClickEvent, CursorStyle, ElementId, FontWeight, Hsla,
    InteractiveElement, IntoElement, SharedString, Window,
};

use super::icon::{icon, Icon};
use crate::theme::{ActiveTheme, Theme};

/// Semantic colour of a status value.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tone {
    /// Muted grey ("Not configured").
    #[default]
    Neutral,
    /// Green ("Installed", "Configured").
    Ok,
    /// Orange ("Unsaved", "Not subscribed").
    Warn,
    /// Red.
    Danger,
}

impl Tone {
    pub fn from_ok(ok: bool) -> Self {
        if ok {
            Tone::Ok
        } else {
            Tone::Neutral
        }
    }
    pub fn color(self, t: &Theme) -> Hsla {
        match self {
            Tone::Neutral => t.text_muted,
            Tone::Ok => t.ok,
            Tone::Warn => t.warning,
            Tone::Danger => t.danger,
        }
    }
}

/// Row with the label left and a dot + value right (Dashboard "Status Overview").
///
/// ```ignore
/// StatusRow::new("T7 Patch", Tone::from_ok(installed)).value("Configured")
/// ```
#[derive(IntoElement)]
pub struct StatusRow {
    label: SharedString,
    value: SharedString,
    tone: Tone,
    last: bool,
    min_h: f32,
    grow: bool,
}

impl StatusRow {
    pub fn new(label: impl Into<SharedString>, tone: Tone) -> Self {
        Self {
            label: label.into(),
            value: "".into(),
            tone,
            last: false,
            min_h: 30.,
            grow: false,
        }
    }
    /// Minimum row height (default 30).
    pub fn min_h(mut self, h: f32) -> Self {
        self.min_h = h;
        self
    }
    /// Share the free height of a column equally with the other growing rows.
    pub fn grow(mut self) -> Self {
        self.grow = true;
        self
    }
    pub fn value(mut self, v: impl Into<SharedString>) -> Self {
        self.value = v.into();
        self
    }
    pub fn last(mut self) -> Self {
        self.last = true;
        self
    }
}

impl RenderOnce for StatusRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let color = self.tone.color(&t);
        let dot = if self.tone == Tone::Neutral {
            gpui::rgb(0xa8b2be).into()
        } else {
            color
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(14.))
            .min_h(px(self.min_h))
            .when(self.grow, |d| d.flex_grow())
            .when(!self.last, |d| d.border_b_1().border_color(t.border))
            .child(div().min_w_0().whitespace_nowrap().text_color(t.text_strong).child(self.label))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .min_w_0()
                    .font_weight(FontWeight::BOLD)
                    .text_color(color)
                    .child(div().size(px(9.)).flex_none().rounded_full().bg(dot))
                    .child(div().truncate().child(self.value)),
            )
    }
}

/// Small tile: muted label over a coloured value (legacy `.status-pill`).
///
/// ```ignore
/// StatusPill::new("Config", "t7patch.conf found", Tone::Ok)
/// ```
#[derive(IntoElement)]
pub struct StatusPill {
    label: SharedString,
    value: SharedString,
    tone: Tone,
    inline: Option<bool>,
    min_h: f32,
}

impl StatusPill {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>, tone: Tone) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            tone,
            inline: None,
            min_h: 56.,
        }
    }
    /// One line: label left, value right (legacy `.dxvk-control-bar .status-pill` when `bordered`,
    /// `.t7-overview-grid .status-pill` when not).
    pub fn inline(mut self, bordered: bool) -> Self {
        self.inline = Some(bordered);
        self
    }
    pub fn min_h(mut self, h: f32) -> Self {
        self.min_h = h;
        self
    }
}

impl RenderOnce for StatusPill {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let value_color = if self.tone == Tone::Neutral { t.text_strong } else { self.tone.color(&t) };
        if let Some(bordered) = self.inline {
            return div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .w_full()
                .min_w_0()
                .min_h(px(self.min_h))
                .px(px(14.))
                .when(bordered, |d| {
                    d.border_1()
                        .border_color(t.border)
                        .rounded(px(t.radius - 4.))
                        .bg(gpui::white().opacity(0.03))
                })
                .child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_size(px(t.font_xs))
                        .font_weight(FontWeight::BOLD)
                        .text_color(t.text_muted)
                        .child(self.label),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_right()
                        .text_size(px(t.font_md))
                        .font_weight(FontWeight::BOLD)
                        .text_color(value_color)
                        .child(self.value),
                )
                .into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .flex_grow()
            .justify_center()
            .gap(px(4.))
            .min_w_0()
            .min_h(px(self.min_h))
            .px(px(14.))
            .py(px(6.))
            .border_1()
            .border_color(t.border)
            .rounded(px(t.radius - 3.))
            .bg(gpui::white().opacity(0.035))
            .child(
                div()
                    .text_size(px(t.font_xs))
                    .font_weight(FontWeight::BOLD)
                    .text_color(t.text_muted)
                    .child(self.label),
            )
            .child(
                // Wraps by word at narrow widths (no truncation).
                div()
                    .text_size(px(t.font_sm))
                    .font_weight(FontWeight::BOLD)
                    .text_color(value_color)
                    .child(self.value),
            )
            .into_any_element()
    }
}

/// Inline `label ........ value` line (legacy `.tool-metric` / key-value details).
///
/// ```ignore
/// Metric::new("Steam ID", "Not found", Tone::Neutral)
/// ```
#[derive(IntoElement)]
#[allow(dead_code)] // kit API
pub struct Metric {
    label: SharedString,
    value: SharedString,
    tone: Tone,
}

impl Metric {
    #[allow(dead_code)] // kit API
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>, tone: Tone) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            tone,
        }
    }
}

impl RenderOnce for Metric {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .min_h(px(26.))
            .child(
                div()
                    .flex_none()
                    .text_size(px(t.font_sm))
                    .text_color(t.text_muted)
                    .child(self.label),
            )
            .child(
                // Wraps by word at narrow widths (no truncation).
                div()
                    .min_w_0()
                    .text_right()
                    .text_size(px(t.font_sm))
                    .font_weight(FontWeight::BOLD)
                    .text_color(if self.tone == Tone::Neutral {
                        t.text_strong
                    } else {
                        self.tone.color(&t)
                    })
                    .child(self.value),
            )
    }
}

/// Rounded pill label (legacy `.option-badge`).
///
/// ```ignore
/// Badge::new("Best for mods").tone(Tone::Neutral)   // Tone::Ok for "Active"
/// ```
#[derive(IntoElement)]
pub struct Badge {
    label: SharedString,
    tone: Tone,
}

impl Badge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            tone: Tone::Neutral,
        }
    }
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let (border, bg, fg) = match self.tone {
            Tone::Neutral => (t.border, gpui::transparent_black(), t.text_muted),
            Tone::Ok => (t.ok_border, t.ok_bg, t.ok),
            Tone::Warn => (t.warning.opacity(0.4), t.warning.opacity(0.14), t.warning),
            Tone::Danger => (t.danger_border, t.danger_bg, t.danger),
        };
        div()
            .flex_none()
            .px(px(9.))
            .py(px(3.))
            .rounded_full()
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_color(fg)
            .text_size(px(t.font_xs))
            .font_weight(FontWeight::BOLD)
            .whitespace_nowrap()
            .child(self.label)
    }
}

/// Full-width message strip (legacy `.error-strip`) with icon and optional dismiss X.
///
/// ```ignore
/// Alert::error("Could not reach backend").on_dismiss(|_, _, cx| ...)
/// ```
#[derive(IntoElement)]
pub struct Alert {
    message: SharedString,
    tone: Tone,
    on_dismiss: Option<super::ClickFn>,
    action: Option<AnyElement>,
}

impl Alert {
    pub fn error(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
            tone: Tone::Danger,
            on_dismiss: None,
            action: None,
        }
    }
    pub fn warning(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
            tone: Tone::Warn,
            on_dismiss: None,
            action: None,
        }
    }
    pub fn on_dismiss(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Box::new(f));
        self
    }
    /// A trailing control (e.g. a small button) before the dismiss X.
    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for Alert {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let (border, bg, fg): (Hsla, Hsla, Hsla) = if self.tone == Tone::Warn {
            (
                t.warning.opacity(0.4),
                t.warning.opacity(0.12),
                gpui::rgb(0xffe6c2).into(),
            )
        } else {
            (t.danger_border, t.danger_bg, gpui::rgb(0xffd8d4).into())
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_h(px(34.))
            .px(px(12.))
            .border_1()
            .border_color(border)
            .rounded(px(9.))
            .bg(bg)
            .text_color(fg)
            .child(icon(Icon::TriangleAlert).size(px(16.)).text_color(fg))
            .child(div().flex_1().min_w_0().child(self.message))
            .children(self.action)
            .when_some(self.on_dismiss, |d, f| {
                d.child(
                    div()
                        .id("alert-dismiss")
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(24.))
                        .rounded(px(5.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(t.hover_strong))
                        .child(icon(Icon::X).size(px(15.)).text_color(fg))
                        .on_click(f),
                )
            })
    }
}

/// Selectable chip with an optional colour swatch (legacy `.color-chip`, gamertag colours).
///
/// ```ignore
/// Chip::new("c-red", "Red").swatch(rgb(0xff453a).into()).active(sel).on_click(..)
/// ```
#[derive(IntoElement)]
pub struct Chip {
    id: ElementId,
    label: SharedString,
    swatch: Option<Hsla>,
    active: bool,
    disabled: bool,
    dim: bool,
    height: f32,
    on_click: Option<super::ClickFn>,
}

impl Chip {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            swatch: None,
            active: false,
            disabled: false,
            dim: false,
            height: 32.,
            on_click: None,
        }
    }
    /// Muted text (legacy `.recommended-chip.secondary`), still brightens when active.
    pub fn dim(mut self, d: bool) -> Self {
        self.dim = d;
        self
    }
    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }
    pub fn swatch(mut self, c: Hsla) -> Self {
        self.swatch = Some(c);
        self
    }
    pub fn active(mut self, a: bool) -> Self {
        self.active = a;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Chip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let active = self.active;
        div()
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(self.height))
            .px(px(10.))
            .border_1()
            .border_color(if active {
                t.accent.opacity(0.42)
            } else {
                t.border
            })
            .rounded(px(t.radius - 4.))
            .bg(if active {
                t.accent.opacity(0.14)
            } else {
                gpui::white().opacity(0.04)
            })
            .text_color(if active {
                t.text
            } else if self.dim {
                t.text_muted
            } else {
                t.text_strong
            })
            .text_size(px(t.font_xs))
            .font_weight(FontWeight::BOLD)
            .tab_index(0)
            .focus(move |s| s.border_color(t.focus_ring))
            .when_some(self.swatch, |d, c| {
                d.child(
                    div()
                        .size(px(12.))
                        .flex_none()
                        .rounded_full()
                        .bg(c)
                        .border_1()
                        .border_color(gpui::white().opacity(0.34)),
                )
            })
            .child(self.label)
            .map(|d| {
                if self.disabled {
                    d.opacity(0.55).cursor(CursorStyle::OperationNotAllowed)
                } else {
                    d.cursor_pointer()
                        .hover(move |s| s.bg(t.hover))
                        .when_some(self.on_click, |d, f| d.on_click(f))
                }
            })
    }
}

/// Convenience: an `AnyElement` icon+text requirement line (`CheckCircle2` green / `TriangleAlert` orange).
///
/// ```ignore
/// requirement("Game detected", detected)
/// ```
#[allow(dead_code)] // kit API
pub fn requirement(label: impl Into<SharedString>, ok: bool, cx: &App) -> AnyElement {
    let t = *cx.theme();
    let color = if ok { t.ok } else { t.warning };
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text_color(color)
        .font_weight(FontWeight::BOLD)
        .child(
            icon(if ok {
                Icon::CircleCheck
            } else {
                Icon::TriangleAlert
            })
            .size(px(15.))
            .text_color(color),
        )
        .child(label.into())
        .into_any_element()
}
