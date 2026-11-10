//! Selection strips: `Segmented` (equal-width segmented control, legacy release-channel switch)
//! and `SubTabs` (legacy `.subtab-bar`: content-width tabs with a red active state).
//! Both are stateless: the owner holds the selected index and updates it in `on_change`.

use gpui::{
    div, prelude::*, px, App, CursorStyle, ElementId, FontWeight, InteractiveElement, IntoElement,
    SharedString, Window,
};

use crate::theme::ActiveTheme;

type ChangeHandler = Box<dyn Fn(usize, &mut Window, &mut App) + 'static>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Segmented,
    Tabs,
    /// `Tabs`, stretched over the whole row.
    TabsFull,
}

fn strip(
    id: ElementId,
    items: Vec<SharedString>,
    selected: usize,
    disabled: bool,
    style: Style,
    on_change: Option<ChangeHandler>,
    cx: &App,
) -> impl IntoElement {
    let t = *cx.theme();
    let full = style == Style::TabsFull;
    let style = if full { Style::Tabs } else { style };
    let on_change = on_change.map(std::rc::Rc::new);
    div()
        .id(id.clone())
        .flex()
        .gap(px(4.))
        .p(px(4.))
        .border_1()
        .border_color(t.border)
        .rounded(px(if style == Style::Segmented {
            t.radius - 5.
        } else {
            t.radius_sm
        }))
        .bg(gpui::white().opacity(0.035))
        .when(style == Style::Tabs && !full, |d| d.flex_none())
        .when(full, |d| d.w_full())
        .when(style == Style::Segmented, |d| d.w_full())
        .children(items.into_iter().enumerate().map(|(i, label)| {
            let active = i == selected;
            let on_change = on_change.clone();
            div()
                .id(("seg-item", i))
                .flex()
                .items_center()
                .justify_center()
                .h(px(if style == Style::Segmented { 26. } else { 34. }))
                .when(style == Style::Segmented || full, |d| d.flex_1().min_w_0())
                .when(style == Style::Tabs && !full, |d| d.px(px(18.)).min_w(px(100.)))
                .rounded(px(t.radius_xs + 1.))
                .border_1()
                .border_color(if active && style == Style::Tabs {
                    t.accent_border
                } else {
                    gpui::transparent_black()
                })
                .bg(if active {
                    if style == Style::Tabs {
                        t.accent_bg
                    } else {
                        t.accent.opacity(0.2)
                    }
                } else {
                    gpui::transparent_black()
                })
                .text_color(if active { t.text } else { t.text_strong })
                .text_size(px(t.font_sm))
                .font_weight(FontWeight::BOLD)
                .whitespace_nowrap()
                .tab_index(0)
                .focus(move |s| s.border_color(t.focus_ring))
                .map(|d| {
                    if disabled {
                        d.opacity(0.7).cursor(CursorStyle::Arrow)
                    } else {
                        d.cursor_pointer()
                            .when(!active, |d| {
                                d.hover(move |s| s.bg(t.hover).text_color(t.text))
                            })
                            .when_some(on_change, move |d, f| {
                                d.on_click(move |_, w, cx| f(i, w, cx))
                            })
                    }
                })
                .child(label)
        }))
}

/// Equal-width segmented control.
///
/// ```ignore
/// Segmented::new("channel", ["Stable", "Beta"]).selected(ix)
///     .on_change(move |i, _w, cx| this.update(cx, |t, cx| { t.channel = i; cx.notify() }))
/// ```
#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    items: Vec<SharedString>,
    selected: usize,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl Segmented {
    pub fn new(
        id: impl Into<ElementId>,
        items: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        Self {
            id: id.into(),
            items: items.into_iter().map(Into::into).collect(),
            selected: 0,
            disabled: false,
            on_change: None,
        }
    }
    pub fn selected(mut self, ix: usize) -> Self {
        self.selected = ix;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn on_change(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        strip(
            self.id,
            self.items,
            self.selected,
            self.disabled,
            Style::Segmented,
            self.on_change,
            cx,
        )
    }
}

/// Content-width tab strip (Graphics Settings / DXVK).
///
/// ```ignore
/// SubTabs::new("gfx-tabs", ["Graphics Settings", "DXVK"]).selected(self.tab).on_change(..)
/// ```
#[derive(IntoElement)]
pub struct SubTabs {
    id: ElementId,
    items: Vec<SharedString>,
    selected: usize,
    full: bool,
    on_change: Option<ChangeHandler>,
}

impl SubTabs {
    pub fn new(
        id: impl Into<ElementId>,
        items: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        Self {
            id: id.into(),
            items: items.into_iter().map(Into::into).collect(),
            selected: 0,
            full: false,
            on_change: None,
        }
    }
    pub fn selected(mut self, ix: usize) -> Self {
        self.selected = ix;
        self
    }
    /// Full-width bar with equal-width halves (legacy `.content-panel > .subtab-bar`).
    pub fn full_width(mut self) -> Self {
        self.full = true;
        self
    }
    pub fn on_change(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }
}

impl RenderOnce for SubTabs {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div().flex().child(strip(
            self.id,
            self.items,
            self.selected,
            false,
            if self.full { Style::TabsFull } else { Style::Tabs },
            self.on_change,
            cx,
        ))
    }
}
