//! Buttons: `Button` (text + optional icon) and `IconButton` (square, icon only).

use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, App, ClickEvent, CursorStyle,
    ElementId, FontWeight, InteractiveElement, IntoElement, SharedString, Window,
};

use super::icon::{icon, Icon};
use super::progress::Spinner;
use crate::theme::ActiveTheme;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonKind {
    /// Red gradient: the one main action of a view.
    Primary,
    /// Neutral raised button (default).
    Secondary,
    /// Red-tinted destructive action.
    Danger,
    /// Raised grey with a faint gradient and a lighter border (legacy `.tool-button.install-primary`).
    Subtle,
    /// Transparent until hovered.
    #[allow(dead_code)]
    Ghost,
}

/// Text button. Focusable (Tab) and activated with Enter/Space.
///
/// ```ignore
/// Button::new("launch", "Launch Game").primary().icon(Icon::SquarePlay)
///     .busy(self.launching)
///     .on_click(cx.listener(|this, _ev, _w, cx| this.launch(cx)))
/// ```
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    kind: ButtonKind,
    icon: Option<Icon>,
    disabled: bool,
    busy: bool,
    small: bool,
    full: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: ButtonKind::Secondary,
            icon: None,
            disabled: false,
            busy: false,
            small: false,
            full: false,
            on_click: None,
        }
    }
    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }
    pub fn primary(self) -> Self {
        self.kind(ButtonKind::Primary)
    }
    pub fn danger(self) -> Self {
        self.kind(ButtonKind::Danger)
    }
    #[allow(dead_code)] // kit API
    pub fn ghost(self) -> Self {
        self.kind(ButtonKind::Ghost)
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Greyed out and inert.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Shows a spinner in place of the icon and ignores clicks (legacy: `busy === "..."`).
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }
    /// 34px high, 12px side padding instead of 36px / 14px (legacy `.small-button`).
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }
    /// Stretch to the parent's width.
    pub fn full_width(mut self) -> Self {
        self.full = true;
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let inert = self.disabled || self.busy;
        let (border, text) = match self.kind {
            ButtonKind::Primary => (t.accent.opacity(0.42), t.text),
            ButtonKind::Secondary => (t.border_strong, t.text),
            ButtonKind::Subtle => (gpui::white().opacity(0.24), t.text),
            ButtonKind::Danger => (t.accent.opacity(0.42), gpui::rgb(0xffd8d4).into()),
            ButtonKind::Ghost => (gpui::transparent_black(), t.text_strong),
        };
        let mut el = div()
            .id(self.id.clone())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(7.))
            .h(px(if self.small { 34. } else { t.control_h }))
            .px(px(if self.small { 12. } else { 14. }))
            .rounded(px(t.radius_sm))
            .border_1()
            .border_color(border)
            .text_color(text)
            .text_size(px(t.font_md))
            .font_weight(FontWeight::BOLD)
            .whitespace_nowrap()
            // Inert buttons are not tab stops (nothing to activate).
            .when(!inert, |d| d.tab_index(0))
            .focus(move |s| {
                s.border_color(if self.kind == ButtonKind::Primary {
                    t.text
                } else {
                    t.focus_ring
                })
            })
            .when(self.full, |d| d.w_full())
            .map(|d| match self.kind {
                ButtonKind::Primary => d.bg(linear_gradient(
                    135.,
                    linear_color_stop(t.accent, 0.),
                    linear_color_stop(t.accent_dark, 1.),
                )),
                ButtonKind::Secondary => d.bg(t.panel_strong),
                ButtonKind::Subtle => d.bg(linear_gradient(
                    135.,
                    linear_color_stop(gpui::white().opacity(0.14), 0.),
                    linear_color_stop(gpui::white().opacity(0.08), 1.),
                )),
                ButtonKind::Danger => d.bg(t.danger_bg),
                ButtonKind::Ghost => d,
            });
        if inert {
            el = el.opacity(0.62).cursor(if self.busy {
                CursorStyle::Arrow
            } else {
                CursorStyle::OperationNotAllowed
            });
        } else {
            el = el
                .cursor_pointer()
                .hover(move |s| match self.kind {
                    ButtonKind::Primary => s.bg(linear_gradient(
                        135.,
                        linear_color_stop(t.accent_hover, 0.),
                        linear_color_stop(t.accent, 1.),
                    )),
                    ButtonKind::Danger => s.bg(t.danger.opacity(0.22)),
                    _ => s
                        .bg(t.hover_strong)
                        .border_color(gpui::white().opacity(0.24)),
                })
                .active(|s| s.mt(px(1.)).mb(px(-1.)))
                .when_some(self.on_click, |d, f| d.on_click(f));
        }
        el.when(self.busy, |d| {
            d.child(Spinner::new("spin").size(px(15.)).color(text))
        })
        .when(!self.busy, |d| {
            d.when_some(self.icon, |d, i| {
                d.child(icon(i).size(px(16.)).text_color(text))
            })
        })
        .child(self.label)
    }
}

/// Square icon-only button (legacy `.icon-action`: close X, show/hide password, ...).
///
/// ```ignore
/// IconButton::new("close", Icon::X).on_click(|_, w, cx| ...)
/// ```
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: Icon,
    disabled: bool,
    active: bool,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: Icon) -> Self {
        Self {
            id: id.into(),
            icon,
            disabled: false,
            active: false,
            on_click: None,
        }
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Highlighted (toggled-on) look.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(34.))
            .rounded(px(9.))
            .border_1()
            .border_color(if self.active {
                t.accent_border
            } else {
                t.border
            })
            .bg(if self.active { t.accent_bg } else { t.panel })
            .tab_index(0)
            .focus(move |s| s.border_color(t.focus_ring))
            .child(icon(self.icon).size(px(16.)).text_color(t.text))
            .map(|d| {
                if self.disabled {
                    d.opacity(0.5).cursor(CursorStyle::OperationNotAllowed)
                } else {
                    d.cursor_pointer()
                        .hover(move |s| s.bg(t.hover_strong))
                        .when_some(self.on_click, |d, f| d.on_click(f))
                }
            })
    }
}
