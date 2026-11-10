//! Form layout helpers. `SettingRow` is the legacy `.setting-row` (label left, control right,
//! hairline below); `FormRow` adds help text and a fixed label column; `Field` is a label
//! stacked over a control (Name / Password fields on the T7 view).

// Kit file: FormRow / Field are documented widget-kit API used by the gallery, not yet by a tab.
#![allow(dead_code)]

use gpui::{
    div, prelude::*, px, AnyElement, App, FontWeight, IntoElement, ParentElement, SharedString,
    Window,
};

use crate::theme::ActiveTheme;

/// Label on the left, control on the right, bottom border (omit with `.last()`).
///
/// ```ignore
/// SettingRow::new("Vertical sync").control(Toggle::new("vsync", on))
/// ```
#[derive(IntoElement)]
pub struct SettingRow {
    label: SharedString,
    help: Option<SharedString>,
    control: Option<AnyElement>,
    last: bool,
    boxed: bool,
    /// `Some(r)`: label column and control column split `r : 1` and the control fills its column.
    fill: Option<f32>,
    /// Control sits right after the label instead of at the far edge (legacy `ToggleRow`).
    start: bool,
}

impl SettingRow {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            help: None,
            control: None,
            last: false,
            boxed: false,
            fill: None,
            start: false,
        }
    }
    /// Label and control columns split `label_ratio : 1` (legacy `.number-row`, `.slider-row`,
    /// quick tiles); the control fills its column.
    pub fn fill(mut self, label_ratio: f32) -> Self {
        self.fill = Some(label_ratio);
        self
    }
    /// Keep the control next to the label (legacy toggle rows), not at the right edge.
    pub fn control_start(mut self) -> Self {
        self.start = true;
        self
    }
    /// Secondary line under the label.
    pub fn help(mut self, h: impl Into<SharedString>) -> Self {
        self.help = Some(h.into());
        self
    }
    pub fn control(mut self, c: impl IntoElement) -> Self {
        self.control = Some(c.into_any_element());
        self
    }
    /// Suppress the bottom hairline.
    pub fn last(mut self) -> Self {
        self.last = true;
        self
    }
    /// Standalone bordered tile (legacy `.graphics-quick-grid .setting-row`).
    pub fn boxed(mut self) -> Self {
        self.boxed = true;
        self
    }
}

impl RenderOnce for SettingRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let (fill, start) = (self.fill, self.start);
        let label_col = div().flex().flex_col().min_w_0();
        let label_col = match (fill, start) {
            (Some(r), _) => crate::views::fr(label_col, r),
            (None, true) => label_col.flex_none(),
            (None, false) => label_col.flex_1(),
        };
        div()
            .flex()
            .items_center()
            .when(fill.is_none() && !start, |d| d.justify_between())
            .gap(px(if self.boxed || fill.is_some() || start { 10. } else { 14. }))
            .min_h(px(if self.boxed { 50. } else { 38. }))
            .when(self.boxed, |d| d.flex_grow())
            .when(fill.is_some(), |d| d.w_full())
            .px(px(14.))
            .py(px(if self.boxed { 4. } else { 7. }))
            .min_w_0()
            .when(!self.last && !self.boxed, |d| {
                d.border_b_1().border_color(t.border)
            })
            .when(self.boxed, |d| {
                d.border_1()
                    .border_color(t.border)
                    .rounded(px(t.radius - 4.))
                    .bg(gpui::white().opacity(0.03))
            })
            .child(
                label_col
                    .child(
                        div()
                            .text_size(px(t.font_md))
                            .text_color(t.text_strong)
                            .child(self.label),
                    )
                    .when_some(self.help, |d, h| {
                        d.child(
                            div()
                                .text_size(px(t.font_xs))
                                .text_color(t.text_muted)
                                .child(h),
                        )
                    }),
            )
            .children(self.control.map(|c| {
                if fill.is_some() {
                    crate::views::fr(div(), 1.0).min_w(px(96.)).flex().flex_col().child(c)
                } else {
                    div().flex_none().child(c)
                }
            }))
    }
}

/// Label column (fixed width) + control that fills the rest; optional help under the control.
///
/// ```ignore
/// FormRow::new("Display Mode").help("Applies on next launch").control(select.clone())
/// ```
#[derive(IntoElement)]
pub struct FormRow {
    label: SharedString,
    help: Option<SharedString>,
    control: Option<AnyElement>,
    label_w: f32,
    last: bool,
}

impl FormRow {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            help: None,
            control: None,
            label_w: 150.,
            last: false,
        }
    }
    pub fn help(mut self, h: impl Into<SharedString>) -> Self {
        self.help = Some(h.into());
        self
    }
    pub fn control(mut self, c: impl IntoElement) -> Self {
        self.control = Some(c.into_any_element());
        self
    }
    #[allow(dead_code)] // kit API
    pub fn label_width(mut self, w: f32) -> Self {
        self.label_w = w;
        self
    }
    pub fn last(mut self) -> Self {
        self.last = true;
        self
    }
}

impl RenderOnce for FormRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .min_h(px(t.row_h + 4.))
            .px(px(14.))
            .py(px(4.))
            .min_w_0()
            .when(!self.last, |d| d.border_b_1().border_color(t.border))
            .child(
                div()
                    .w(px(self.label_w))
                    .flex_none()
                    .text_size(px(t.font_md))
                    .text_color(t.text_strong)
                    .child(self.label),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(3.))
                    .children(self.control)
                    .when_some(self.help, |d, h| {
                        d.child(
                            div()
                                .text_size(px(t.font_xs))
                                .text_color(t.text_muted)
                                .child(h),
                        )
                    }),
            )
    }
}

/// Small bold muted caption above a control.
///
/// ```ignore
/// Field::new("Name").child(input.clone())
/// ```
#[derive(IntoElement)]
pub struct Field {
    label: SharedString,
    children: Vec<AnyElement>,
}

impl Field {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            children: Vec::new(),
        }
    }
}

impl ParentElement for Field {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Field {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .min_w_0()
            .child(
                div()
                    .text_size(px(t.font_xs))
                    .font_weight(FontWeight::BOLD)
                    .text_color(t.text_muted)
                    .child(self.label),
            )
            .children(self.children)
    }
}
