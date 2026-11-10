//! On/off controls: `Toggle` (switch), `Checkbox`, `Radio`. All stateless: the owner keeps the
//! value and flips it in `on_change`. Focusable; Space/Enter toggles.

use gpui::{
    div, prelude::*, px, App, ClickEvent, CursorStyle, ElementId, InteractiveElement, IntoElement,
    SharedString, Window,
};

use super::icon::{icon, Icon};
use crate::theme::ActiveTheme;

type ChangeHandler = Box<dyn Fn(bool, &mut Window, &mut App) + 'static>;

/// Pill switch (52x28 in the legacy CSS, green when on).
///
/// ```ignore
/// let this = cx.entity();
/// Toggle::new("vsync", self.vsync).disabled(!ready)
///     .on_change(move |v, _w, cx| this.update(cx, |t, cx| { t.vsync = v; cx.notify() }))
/// ```
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    checked: bool,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            disabled: false,
            on_change: None,
        }
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let checked = self.checked;
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .w(px(46.))
            .h(px(26.))
            .px(px(3.))
            .rounded_full()
            .border_1()
            .border_color(if checked {
                t.ok_border
            } else {
                t.border_strong
            })
            .bg(if checked { t.ok_bg } else { t.field })
            .when(checked, |d| d.justify_end())
            .tab_index(0)
            .focus(move |s| s.border_color(t.focus_ring))
            .child(
                div()
                    .size(px(18.))
                    .rounded_full()
                    .bg(if checked { t.ok } else { t.text_muted }),
            )
            .map(|d| {
                if self.disabled {
                    d.opacity(0.55).cursor(CursorStyle::OperationNotAllowed)
                } else {
                    d.cursor_pointer().when_some(self.on_change, move |d, f| {
                        d.on_click(move |_: &ClickEvent, w, cx| f(!checked, w, cx))
                    })
                }
            })
    }
}

fn box_shell(
    id: ElementId,
    label: Option<SharedString>,
    disabled: bool,
    mark: impl IntoElement,
    on_click: Option<super::ClickFn>,
    cx: &App,
) -> impl IntoElement {
    let t = *cx.theme();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .min_h(px(32.))
        .text_color(t.text)
        .text_size(px(t.font_md))
        .tab_index(0)
        .rounded(px(t.radius_xs))
        .focus(move |s| s.bg(t.hover))
        .child(mark)
        .when_some(label, |d, l| d.child(div().min_w_0().child(l)))
        .map(|d| {
            if disabled {
                d.opacity(0.55).cursor(CursorStyle::OperationNotAllowed)
            } else {
                d.cursor_pointer().when_some(on_click, |d, f| d.on_click(f))
            }
        })
}

/// Square checkbox with optional label (legacy `.check-row`).
///
/// ```ignore
/// Checkbox::new("d3d", state.qol.d3dcompiler).label("Use latest d3dcompiler").on_change(..)
/// ```
#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    checked: bool,
    label: Option<SharedString>,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            label: None,
            disabled: false,
            on_change: None,
        }
    }
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
    #[allow(dead_code)] // kit API
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let checked = self.checked;
        let mark = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(18.))
            .rounded(px(4.))
            .border_1()
            .border_color(if checked { t.accent } else { t.border_strong })
            .bg(if checked { t.accent } else { t.field })
            .when(checked, |d| {
                d.child(icon(Icon::Check).size(px(13.)).text_color(t.text))
            });
        let f = self.on_change;
        box_shell(
            self.id,
            self.label,
            self.disabled,
            mark,
            f.map(|f| {
                Box::new(move |_: &ClickEvent, w: &mut Window, cx: &mut App| f(!checked, w, cx))
                    as _
            }),
            cx,
        )
    }
}

/// Radio button with optional label. `on_select` fires when an unselected radio is clicked;
/// the owner updates the selected id (legacy launch-profile list).
///
/// ```ignore
/// Radio::new(("profile", i), sel == i).label("Play Offline").on_select(..)
/// ```
#[derive(IntoElement)]
pub struct Radio {
    id: ElementId,
    selected: bool,
    label: Option<SharedString>,
    disabled: bool,
    on_select: Option<super::ActionFn>,
}

impl Radio {
    pub fn new(id: impl Into<ElementId>, selected: bool) -> Self {
        Self {
            id: id.into(),
            selected,
            label: None,
            disabled: false,
            on_select: None,
        }
    }
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
    #[allow(dead_code)] // kit API
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn on_select(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Radio {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let sel = self.selected;
        let mark = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(18.))
            .rounded_full()
            .border_1()
            .border_color(if sel { t.accent } else { t.border_strong })
            .bg(t.field)
            .when(sel, |d| {
                d.child(div().size(px(8.)).rounded_full().bg(t.accent))
            });
        let f = self.on_select;
        box_shell(
            self.id,
            self.label,
            self.disabled,
            mark,
            f.map(|f| {
                Box::new(move |_: &ClickEvent, w: &mut Window, cx: &mut App| {
                    if !sel {
                        f(w, cx)
                    }
                }) as _
            }),
            cx,
        )
    }
}
