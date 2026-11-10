//! Containers: `Panel` (heading above a bordered body), `Card` (icon title inside a subtle box),
//! `Group` (sub-section heading used inside panels), `Collapsible` (legacy `<details>`),
//! `Divider`, and `DropZone`.
//!
//! All are `ParentElement`s: add content with `.child(..)` / `.children(..)`.

use gpui::{
    div, prelude::*, px, AnyElement, App, ClickEvent, CursorStyle, ElementId, ExternalPaths,
    FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString, Window,
};

use super::icon::{icon, Icon};
use crate::theme::ActiveTheme;

/// Heading (`--font-lg`, bold) above a bordered, translucent body. The body stretches in a
/// flex column; give the Panel `.flex_1().min_h_0()` to let it fill remaining space.
///
/// ```ignore
/// Panel::new("Status Overview").child(StatusRow::new("T7 Patch", ok))
/// ```
#[derive(IntoElement)]
pub struct Panel {
    title: Option<SharedString>,
    /// Body padding (x, y) in px.
    pad: (f32, f32),
    /// Minimum height; grows into free space of the parent column (see [`Panel::fill`]).
    fill: Option<f32>,
    children: Vec<AnyElement>,
}

impl Panel {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: Some(title.into()),
            pad: (14., 14.),
            fill: None,
            children: Vec::new(),
        }
    }
    /// No heading, just the bordered body.
    #[allow(dead_code)] // kit API
    pub fn untitled() -> Self {
        Self {
            title: None,
            pad: (14., 14.),
            fill: None,
            children: Vec::new(),
        }
    }
    /// Remove the body padding (for lists that want to run edge to edge).
    pub fn flush(mut self) -> Self {
        self.pad = (0., 0.);
        self
    }
    /// Body padding: `x` left/right, `y` top/bottom.
    pub fn padding(mut self, x: f32, y: f32) -> Self {
        self.pad = (x, y);
        self
    }
    /// Legacy `flex: 1; min-height: N`: at least `min_h` high, and stretches to fill a parent that is
    /// itself a growing flex column (`div().flex().flex_col().flex_grow().flex_shrink_0()`; never
    /// shrinks, so a short window still scrolls instead of clipping). Children that want to spread
    /// out use `.flex_1()`.
    pub fn fill(mut self, min_h: f32) -> Self {
        self.fill = Some(min_h);
        self
    }
}

impl ParentElement for Panel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Panel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .min_h_0()
            .when_some(self.fill, |d, min_h| d.flex_grow().min_h(px(min_h)))
            .when_some(self.title, |d, title| {
                d.child(
                    div()
                        .mb(px(8.))
                        .text_size(px(t.font_lg))
                        .font_weight(FontWeight::BOLD)
                        .text_color(t.text)
                        .child(title),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(t.border)
                    .rounded(px(t.radius))
                    .bg(t.panel)
                    .overflow_hidden()
                    .px(px(self.pad.0))
                    .py(px(self.pad.1))
                    .children(self.children),
            )
    }
}

/// Subtle bordered box with an optional icon + title (legacy `.tool-card`).
///
/// ```ignore
/// Card::new().icon(Icon::Cpu).title("System").child(Metric::new("Platform", "linux"))
/// ```
#[derive(IntoElement)]
pub struct Card {
    title: Option<SharedString>,
    icon: Option<Icon>,
    children: Vec<AnyElement>,
}

impl Card {
    pub fn new() -> Self {
        Self {
            title: None,
            icon: None,
            children: Vec::new(),
        }
    }
    pub fn title(mut self, t: impl Into<SharedString>) -> Self {
        self.title = Some(t.into());
        self
    }
    pub fn icon(mut self, i: Icon) -> Self {
        self.icon = Some(i);
        self
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Card {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .min_w_0()
            .p(px(10.))
            .border_1()
            .border_color(t.border)
            .rounded(px(t.radius - 2.))
            .bg(gpui::white().opacity(0.025))
            .when(self.title.is_some() || self.icon.is_some(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        .text_size(px(t.font_md))
                        .text_color(t.text)
                        .when_some(self.icon, |d, i| d.child(icon(i).text_color(t.text_muted)))
                        .children(self.title),
                )
            })
            .children(self.children)
    }
}

/// Sub-section: small bold heading + a column of rows (legacy `.graphics-settings-group`).
///
/// ```ignore
/// Group::new("Performance").child(SettingRow::new("Vertical sync", toggle))
/// ```
#[derive(IntoElement)]
pub struct Group {
    title: SharedString,
    children: Vec<AnyElement>,
}

impl Group {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            children: Vec::new(),
        }
    }
}

impl ParentElement for Group {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Group {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .border_1()
            .border_color(t.border)
            .rounded(px(t.radius - 3.))
            .bg(gpui::white().opacity(0.03))
            .overflow_hidden()
            .child(
                div()
                    .px(px(14.))
                    .pt(px(14.))
                    .pb(px(7.))
                    .text_size(px(t.font_md))
                    .text_color(t.text)
                    .child(self.title),
            )
            .children(self.children)
    }
}

/// 1px horizontal rule.
#[allow(dead_code)] // kit API
pub fn divider(cx: &App) -> impl IntoElement {
    div().h(px(1.)).w_full().flex_none().bg(cx.theme().border)
}

/// Disclosure section (legacy `<details>/<summary>`): the owner keeps `open`.
///
/// ```ignore
/// Collapsible::new("adv", "Advanced", self.adv_open)
///     .detail("Config, CPU, latency, and VRAM controls")
///     .on_toggle(move |open, _w, cx| this.update(cx, |t, cx| { t.adv_open = open; cx.notify() }))
///     .child(..)
/// ```
#[derive(IntoElement)]
pub struct Collapsible {
    id: ElementId,
    title: SharedString,
    detail: Option<SharedString>,
    open: bool,
    chevron_end: bool,
    on_toggle: Option<super::BoolFn>,
    children: Vec<AnyElement>,
}

impl Collapsible {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>, open: bool) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            detail: None,
            open,
            chevron_end: false,
            on_toggle: None,
            children: Vec::new(),
        }
    }
    pub fn detail(mut self, d: impl Into<SharedString>) -> Self {
        self.detail = Some(d.into());
        self
    }
    /// Put the chevron at the right edge (legacy graphics "Advanced" drawer) instead of before the title.
    pub fn chevron_end(mut self) -> Self {
        self.chevron_end = true;
        self
    }
    pub fn on_toggle(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(f));
        self
    }
}

impl ParentElement for Collapsible {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Collapsible {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let open = self.open;
        let chevron_end = self.chevron_end;
        let chevron = move || {
            icon(match (chevron_end, open) {
                (false, true) => Icon::ChevronDown,
                (false, false) => Icon::ChevronRight,
                (true, true) => Icon::ChevronUp,
                (true, false) => Icon::ChevronDown,
            })
            .text_color(t.text_muted)
        };
        div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(t.border)
            .rounded(px(t.radius - 3.))
            .bg(gpui::white().opacity(0.03))
            .child(
                div()
                    .id(self.id)
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .min_h(px(44.))
                    .px(px(14.))
                    .cursor_pointer()
                    .tab_index(0)
                    .rounded(px(t.radius - 3.))
                    .focus(move |s| s.bg(t.hover))
                    .hover(move |s| s.bg(t.hover))
                    .when(!chevron_end, |d| d.child(chevron()))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .when(chevron_end, |d| d.flex_1().min_w_0())
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(t.font_md))
                                    .child(self.title),
                            )
                            .when_some(self.detail, |d, s| {
                                d.child(
                                    div()
                                        .text_size(px(t.font_xs))
                                        .text_color(t.text_muted)
                                        .child(s),
                                )
                            }),
                    )
                    .when(chevron_end, |d| d.child(chevron()))
                    .when_some(self.on_toggle, move |d, f| {
                        d.on_click(move |_: &ClickEvent, w, cx| f(!open, w, cx))
                    }),
            )
            .when(open, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .p(px(12.))
                        .gap(px(4.))
                        .children(self.children),
                )
            })
    }
}

/// Dashed drop target for files dragged from the OS file manager (Enhanced "Drop DUMP.zip").
/// `on_drop` receives the dropped paths.
///
/// ```ignore
/// DropZone::new("dump-drop", "Drop DUMP.zip or extracted folder here")
///     .detail("or browse to the dump source manually")
///     .on_drop(|paths, _w, cx| { /* paths: &[PathBuf] */ })
/// ```
#[derive(IntoElement)]
pub struct DropZone {
    id: ElementId,
    title: SharedString,
    detail: Option<SharedString>,
    on_drop: Option<super::PathsFn>,
}

impl DropZone {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            detail: None,
            on_drop: None,
        }
    }
    pub fn detail(mut self, d: impl Into<SharedString>) -> Self {
        self.detail = Some(d.into());
        self
    }
    pub fn on_drop(
        mut self,
        f: impl Fn(&[std::path::PathBuf], &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_drop = Some(Box::new(f));
        self
    }
}

impl RenderOnce for DropZone {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        div()
            .id(self.id)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .flex_grow()
            .min_h(px(106.))
            .p(px(18.))
            .border_1()
            .border_dashed()
            .border_color(gpui::white().opacity(0.2))
            .rounded(px(t.radius))
            .bg(gpui::white().opacity(0.02))
            .cursor(CursorStyle::Arrow)
            .drag_over::<ExternalPaths>(move |s, _, _, _| s.border_color(t.accent).bg(t.accent_bg))
            .when_some(self.on_drop, |d, f| {
                d.on_drop(move |paths: &ExternalPaths, w, cx| f(paths.paths(), w, cx))
            })
            // Legacy: a two-row grid, each row centres its text, so the title sits in the upper
            // half and the detail in the lower half when the zone is stretched.
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_1()
                    .w_full()
                    .min_w_0()
                    .font_weight(FontWeight::BOLD)
                    .text_color(t.text)
                    .child(div().w_full().min_w_0().text_center().child(self.title)),
            )
            .when_some(self.detail, |d, s| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_1()
                        .w_full()
                        .min_w_0()
                        .text_size(px(t.font_sm))
                        .font_weight(FontWeight::BOLD)
                        .text_color(t.text_muted)
                        .child(div().w_full().min_w_0().text_center().child(s)),
                )
            })
    }
}
