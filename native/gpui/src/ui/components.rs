//! Small building blocks that reproduce the Electron app's controls
//! (`.tool-button`, `.panel`, `.status-row`, `.toggle`, `.check-row`, ...).
use super::theme;
use gpui::{prelude::*, *};

pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub fn handler(f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> ClickHandler {
    Box::new(f)
}

/// Lucide icons bundled in `assets/icons` (see `assets.rs`).
#[derive(Clone, Copy)]
pub enum Glyph {
    Alert,
    Check,
    CheckCircle,
    ChevronDown,
    ChevronRight,
    Clipboard,
    Close,
    Cpu,
    Dashboard,
    Download,
    Eraser,
    ExternalLink,
    EyeOff,
    FolderOpen,
    Gem,
    Image,
    Key,
    Play,
    Refresh,
    Rotate,
    Save,
    Shield,
    Trash,
    Wrench,
}

impl Glyph {
    fn path(self) -> &'static str {
        match self {
            Self::Alert => "icons/triangle-alert.svg",
            Self::Check => "icons/check.svg",
            Self::CheckCircle => "icons/circle-check.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::Clipboard => "icons/clipboard.svg",
            Self::Close => "icons/x.svg",
            Self::Cpu => "icons/cpu.svg",
            Self::Dashboard => "icons/layout-dashboard.svg",
            Self::Download => "icons/download.svg",
            Self::Eraser => "icons/eraser.svg",
            Self::ExternalLink => "icons/external-link.svg",
            Self::EyeOff => "icons/eye-off.svg",
            Self::FolderOpen => "icons/folder-open.svg",
            Self::Gem => "icons/gem.svg",
            Self::Image => "icons/image.svg",
            Self::Key => "icons/key-round.svg",
            Self::Play => "icons/square-play.svg",
            Self::Refresh => "icons/refresh-cw.svg",
            Self::Rotate => "icons/rotate-ccw.svg",
            Self::Save => "icons/save.svg",
            Self::Shield => "icons/shield-check.svg",
            Self::Trash => "icons/trash-2.svg",
            Self::Wrench => "icons/wrench.svg",
        }
    }
}

pub fn icon(glyph: Glyph, size: f32, color: Hsla) -> Svg {
    svg()
        .path(glyph.path())
        .size(px(size))
        .flex_none()
        .text_color(color)
}

pub fn text_xs(label: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .text_size(px(theme::FONT_XS))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(label.into())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Primary,
    Danger,
}

/// `.tool-button` / `.small-button`.
#[derive(IntoElement)]
pub struct Btn {
    id: ElementId,
    label: SharedString,
    glyph: Option<Glyph>,
    tone: Tone,
    compact: bool,
    tiny: bool,
    grow: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl Btn {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            glyph: None,
            tone: Tone::Plain,
            compact: false,
            tiny: false,
            grow: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.glyph = Some(glyph);
        self
    }

    pub fn primary(mut self) -> Self {
        self.tone = Tone::Primary;
        self
    }

    pub fn danger(mut self) -> Self {
        self.tone = Tone::Danger;
        self
    }

    /// The shorter `.small-button` variant.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// The 24px variant used in the title bar.
    pub fn tiny(mut self) -> Self {
        self.tiny = true;
        self
    }

    /// Share the row evenly with sibling buttons.
    pub fn grow(mut self) -> Self {
        self.grow = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Btn {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let (fill, border, hover_fill, hover_border, color) = match self.tone {
            Tone::Primary => (
                linear_gradient(
                    135.,
                    linear_color_stop(theme::accent(), 0.),
                    linear_color_stop(theme::accent_dark(), 1.),
                ),
                theme::accent_alpha(0.42),
                theme::accent(),
                theme::accent_alpha(0.7),
                theme::text(),
            ),
            Tone::Danger => (
                solid_background(theme::danger_alpha(0.12)),
                theme::danger_alpha(0.34),
                theme::danger_alpha(0.2),
                theme::danger_alpha(0.5),
                theme::danger_text(),
            ),
            Tone::Plain => (
                solid_background(theme::panel_strong()),
                theme::border_strong(),
                theme::white(0.12),
                theme::white(0.24),
                theme::text(),
            ),
        };
        let enabled = !self.disabled;
        let handler = self.on_click.filter(|_| enabled);
        div()
            .id(self.id)
            .flex()
            .items_center()
            .justify_center()
            .gap(px(7.))
            .min_w_0()
            .min_h(px(if self.tiny {
                24.
            } else if self.compact {
                34.
            } else {
                theme::CONTROL_HEIGHT
            }))
            .px(px(if self.tiny {
                8.
            } else if self.compact {
                10.
            } else {
                14.
            }))
            .border_1()
            .border_color(border)
            .rounded(theme::radius_control())
            .bg(fill)
            .text_color(color)
            .text_size(px(if self.tiny {
                theme::FONT_XS
            } else if self.compact {
                theme::FONT_SM
            } else {
                theme::FONT_MD
            }))
            .font_weight(FontWeight::BOLD)
            .when(self.grow, |this| this.flex_1())
            .when_some(self.glyph, |this, glyph| {
                this.child(icon(glyph, if self.compact { 15. } else { 16. }, color))
            })
            // `white-space: normal` on `.small-button`: when equal-width buttons
            // share a narrow row the label wraps instead of being clipped. GPUI
            // only wraps text inside a definite width, hence `w_full`; content-
            // sized buttons keep a bare label (a `w_full` child there collapses).
            .child(if self.grow && self.glyph.is_none() {
                div()
                    .w_full()
                    .text_center()
                    .child(self.label)
                    .into_any_element()
            } else {
                self.label.into_any_element()
            })
            .when(!enabled, |this| this.opacity(0.62).cursor_not_allowed())
            .when(enabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(hover_fill).border_color(hover_border))
            })
            .when_some(handler, |this, handler| this.on_click(handler))
    }
}

/// `.icon-action`: a square icon-only button.
pub fn icon_button(
    id: impl Into<ElementId>,
    glyph: Glyph,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .size(px(theme::CONTROL_HEIGHT))
        .border_1()
        .border_color(theme::border_strong())
        .rounded(theme::radius_control())
        .bg(theme::panel_strong())
        .child(icon(glyph, 15., theme::text()))
        .when(!enabled, |this| this.opacity(0.62).cursor_not_allowed())
        .when(enabled, |this| {
            this.cursor_pointer()
                .hover(|style| style.bg(theme::white(0.12)))
                .on_click(on_click)
        })
}

/// `.panel`: a 16px heading above a bordered body.
pub fn panel(title: impl Into<SharedString>, body: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .min_w_0()
        .child(
            div()
                .mb(px(8.))
                .text_size(px(theme::FONT_LG))
                .font_weight(FontWeight::BOLD)
                .child(title.into()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .border_1()
                .border_color(theme::border())
                .rounded(theme::radius())
                .bg(theme::panel())
                .overflow_hidden()
                .child(body),
        )
}

/// A heading above a body that has no surrounding chrome (the EXE swapper,
/// Enhanced, Graphics and DXVK panels draw their own cards).
pub fn bare_panel(title: impl Into<SharedString>, body: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .min_w_0()
        .child(
            div()
                .mb(px(8.))
                .text_size(px(theme::FONT_LG))
                .font_weight(FontWeight::BOLD)
                .child(title.into()),
        )
        .child(body)
}

/// `.enhanced-card` / `.tool-card` / `.graphics-settings-group`.
pub fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .min_w_0()
        .p(px(12.))
        .border_1()
        .border_color(theme::border())
        .rounded(theme::radius_card())
        .bg(theme::white(0.025))
}

pub fn card_title(glyph: Option<Glyph>, title: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(9.))
        .text_size(px(theme::FONT_MD))
        .font_weight(FontWeight::BOLD)
        .when_some(glyph, |this, glyph| {
            this.child(icon(glyph, 16., theme::muted()))
        })
        .child(title.into())
}

/// A row of equally sized columns that wraps when the window is narrow,
/// approximating `grid-template-columns: repeat(auto-fit, minmax(..))`.
pub fn columns() -> Div {
    div().flex().flex_wrap().gap(px(theme::PANEL_GAP)).w_full()
}

/// A flex child that participates in [`columns`].
pub fn column(min_width: f32) -> Div {
    div().flex().flex_col().flex_1().min_w(px(min_width))
}

pub fn hairline() -> Div {
    div().h(px(1.)).w_full().flex_none().bg(theme::border())
}

/// `.toggle`: the 52x28 pill switch.
pub fn switch(
    id: impl Into<ElementId>,
    on: bool,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex_none()
        .w(px(52.))
        .h(px(28.))
        .rounded_full()
        .border_1()
        .border_color(if on {
            theme::ok_alpha(0.38)
        } else {
            theme::border_strong()
        })
        .bg(if on {
            theme::ok_alpha(0.16)
        } else {
            theme::field()
        })
        .child(
            div()
                .absolute()
                .top(px(4.))
                .left(px(if on { 28. } else { 4. }))
                .size(px(18.))
                .rounded_full()
                .bg(if on { theme::ok() } else { theme::muted() }),
        )
        .when(!enabled, |this| this.opacity(0.62).cursor_not_allowed())
        .when(enabled, |this| this.cursor_pointer().on_click(on_click))
}

fn check_box(checked: bool) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .size(px(18.))
        .rounded(px(4.))
        .border_1()
        .border_color(if checked {
            theme::accent()
        } else {
            theme::white(0.5)
        })
        .bg(if checked {
            theme::accent()
        } else {
            theme::field()
        })
        .when(checked, |this| {
            this.child(icon(Glyph::Check, 14., theme::white(1.)))
        })
}

/// `.check-row`.
pub fn check_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .min_h(px(34.))
        .min_w_0()
        .text_size(px(theme::FONT_MD))
        .font_weight(FontWeight::MEDIUM)
        .child(check_box(checked))
        .child(div().flex_1().min_w_0().child(label.into()))
        .when(!enabled, |this| this.opacity(0.62).cursor_not_allowed())
        .when(enabled, |this| this.cursor_pointer().on_click(on_click))
}

/// `.radio-row`, with the optional trailing state label (`.profile-state`).
pub fn radio_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    trailing: Option<(SharedString, Hsla)>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .min_h(px(30.))
        .min_w_0()
        .border_b_1()
        .border_color(theme::border())
        .text_size(px(theme::FONT_MD))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .on_click(on_click)
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .flex_none()
                .size(px(18.))
                .rounded_full()
                .border_1()
                .border_color(if selected {
                    theme::accent()
                } else {
                    theme::white(0.5)
                })
                .bg(theme::field())
                .when(selected, |this| {
                    this.child(div().size(px(10.)).rounded_full().bg(theme::accent()))
                }),
        )
        .child(div().flex_1().min_w_0().child(label.into()))
        .when_some(trailing, |this, (state, color)| {
            this.child(
                div()
                    .ml_auto()
                    .min_w_0()
                    .truncate()
                    .text_size(px(theme::FONT_SM))
                    .text_color(color)
                    .child(state),
            )
        })
}

/// `.status-row`: a label with a coloured status dot on the right.
pub fn status_row(label: &str, ok: bool, detail: Option<&str>, last: bool) -> Div {
    let shown = if ok {
        detail.unwrap_or("Configured")
    } else {
        "Not configured"
    };
    let tone = if ok { theme::ok() } else { theme::muted() };
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .min_h(px(34.))
        .text_size(px(theme::FONT_MD))
        .text_color(theme::muted_strong())
        .when(!last, |this| {
            this.border_b_1().border_color(theme::border())
        })
        .child(div().flex_1().min_w_0().child(label.to_owned()))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(7.))
                .min_w_0()
                .font_weight(FontWeight::BOLD)
                .text_color(tone)
                .child(div().size(px(9.)).flex_none().rounded_full().bg(if ok {
                    theme::ok()
                } else {
                    theme::dot_idle()
                }))
                .child(div().truncate().child(shown.to_owned())),
        )
}

/// `.module-row`: icon, title and a configured/not configured label.
pub fn module_row(glyph: Glyph, title: &str, active: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .min_h(px(46.))
        .text_size(px(theme::FONT_MD))
        .child(icon(glyph, 22., theme::text()))
        .child(div().flex_1().min_w_0().child(title.to_owned()))
        .child(
            div()
                .font_weight(FontWeight::BOLD)
                .text_color(if active { theme::ok() } else { theme::muted() })
                .child(if active {
                    "Configured"
                } else {
                    "Not configured"
                }),
        )
}

/// `.status-pill`: a small label above a value inside a bordered box.
pub fn status_pill(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    ok: bool,
) -> Div {
    div()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(4.))
        .flex_1()
        .min_w_0()
        .min_h(px(56.))
        .px(px(14.))
        .py(px(8.))
        .border_1()
        .border_color(theme::border())
        .rounded(theme::radius_control())
        .bg(theme::white(0.035))
        .child(text_xs(label, theme::muted()).truncate())
        .child(
            div()
                .truncate()
                .text_size(px(theme::FONT_SM))
                .font_weight(FontWeight::BOLD)
                .text_color(if ok {
                    theme::ok()
                } else {
                    theme::muted_strong()
                })
                .child(value.into()),
        )
}

/// The borderless label-left / value-right variant used inside the T7
/// overview and DXVK control bars.
pub fn inline_pill(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    ok: bool,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .min_w_0()
        .px(px(14.))
        .min_h(px(54.))
        .child(text_xs(label, theme::muted()).truncate())
        .child(
            div()
                .truncate()
                .text_size(px(theme::FONT_SM))
                .font_weight(FontWeight::BOLD)
                .text_color(if ok {
                    theme::ok()
                } else {
                    theme::muted_strong()
                })
                .child(value.into()),
        )
}

/// `.enhanced-metric` / `.t7-state-grid > div`: small label, value below.
pub fn metric(label: impl Into<SharedString>, value: impl Into<SharedString>, tone: Hsla) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .flex_1()
        .min_w_0()
        .child(text_xs(label, theme::muted()))
        .child(
            div()
                .truncate()
                .text_size(px(theme::FONT_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tone)
                .child(value.into()),
        )
}

/// `.t7-state-grid > div`: a [`metric`] inside its own bordered box.
pub fn boxed_metric(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    tone: Hsla,
) -> Div {
    metric(label, value, tone)
        .p(px(10.))
        .border_1()
        .border_color(theme::border())
        .rounded(theme::radius_card())
        .bg(theme::white(0.026))
}

/// `.tool-metric`: a muted label with its value on the same line.
pub fn tool_metric(label: &str, value: impl Into<SharedString>, ok: bool) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap(px(10.))
        .min_w_0()
        .text_size(px(theme::FONT_SM))
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::muted())
                .child(label.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::BOLD)
                .text_color(if ok {
                    gpui::rgb(0x24ff66).into()
                } else {
                    theme::text()
                })
                .child(value.into()),
        )
}

/// `.segmented-control` / `.subtab-bar`: mutually exclusive options.
pub fn segmented(items: Vec<(SharedString, bool, ClickHandler)>, enabled: bool) -> Div {
    div()
        .flex()
        .gap(px(4.))
        .p(px(4.))
        .min_w_0()
        .border_1()
        .border_color(theme::border())
        .rounded(theme::radius_control())
        .bg(theme::white(0.035))
        .children(
            items
                .into_iter()
                .enumerate()
                .map(|(index, (label, active, handler))| {
                    div()
                        .id(("segment", index))
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_1()
                        .min_w_0()
                        .min_h(px(30.))
                        .px(px(10.))
                        .rounded(px(5.))
                        .border_1()
                        .border_color(if active {
                            theme::accent_alpha(0.28)
                        } else {
                            theme::white(0.)
                        })
                        .bg(if active {
                            theme::accent_alpha(0.16)
                        } else {
                            theme::white(0.)
                        })
                        .text_size(px(theme::FONT_SM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(if active {
                            theme::text()
                        } else {
                            theme::muted_strong()
                        })
                        .child(label)
                        .when(!enabled, |this| this.opacity(0.62).cursor_not_allowed())
                        .when(enabled, |this| {
                            this.cursor_pointer()
                                .hover(|style| style.bg(theme::white(0.06)))
                                .on_click(handler)
                        })
                }),
        )
}

/// `.color-chip` / `.recommended-chip`.
pub fn chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    swatch: Option<Hsla>,
    active: bool,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(5.))
        .min_w_0()
        .min_h(px(32.))
        .px(px(6.))
        .border_1()
        .border_color(if active {
            theme::accent_alpha(0.42)
        } else {
            theme::border()
        })
        .rounded(theme::radius_card())
        .bg(if active {
            theme::accent_alpha(0.14)
        } else {
            theme::white(0.04)
        })
        .text_size(px(theme::FONT_XS))
        .font_weight(FontWeight::BOLD)
        .text_color(if active {
            theme::text()
        } else {
            theme::muted_strong()
        })
        .when_some(swatch, |this, color| {
            this.child(
                div()
                    .size(px(12.))
                    .flex_none()
                    .rounded_full()
                    .border_1()
                    .border_color(theme::white(0.34))
                    .bg(color),
            )
        })
        .child(div().truncate().child(label.into()))
        .when(!enabled, |this| this.opacity(0.62).cursor_not_allowed())
        .when(enabled, |this| {
            this.cursor_pointer()
                .hover(|style| style.bg(theme::accent_alpha(0.1)))
                .on_click(on_click)
        })
}

/// A dimmed uppercase-free field caption (`.field-grid label`).
pub fn field_label(label: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::FONT_XS))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::muted())
        .child(label.into())
}

/// The scrim and centring wrapper used by modal dialogs.
pub fn modal(content: impl IntoElement) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(px(28.))
        .bg(theme::scrim())
        .occlude()
        .child(content)
}

/// `.modal` surface: rounded, red glow in the corner approximated by a border.
pub fn modal_surface() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .w(px(520.))
        .max_w_full()
        .p(px(20.))
        .border_1()
        .border_color(theme::border_strong())
        .rounded(px(16.))
        .bg(theme::modal_surface())
        .shadow_lg()
}
