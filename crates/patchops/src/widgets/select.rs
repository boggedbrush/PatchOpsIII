//! Dropdown select (legacy `<select>`): trigger field + popup list, keyboard navigation,
//! closes on outside click / Escape / focus loss.

use gpui::{
    actions, anchored, canvas, deferred, div, point, prelude::*, px, App, Bounds, Context, Corner,
    CursorStyle, EventEmitter, FocusHandle, Focusable, FontWeight, KeyBinding, MouseButton,
    MouseDownEvent, Pixels, Render, ScrollHandle, SharedString, Subscription, TextRun, Window,
};

use super::icon::{icon, Icon};
use super::scroll::Scrollbar;
use super::ellipsis;
use crate::theme::ActiveTheme;

actions!(patchops_select, [Up, Down, Confirm, Cancel, Open]);
const CONTEXT: &str = "PatchOpsSelect";
const ROW_H: f32 = 32.0;
const MAX_VISIBLE: usize = 8;
/// Popup width beyond the widest label: list padding (2x3) + border (2) + row padding (2x10) +
/// check mark and its gap (14 + 8) + scrollbar lane.
const POPUP_CHROME: f32 = 6. + 2. + 20. + 22. + 6.;

/// Width in px of `text` in the bold UI font at `size` (what the popup rows render).
fn text_width(window: &Window, text: &SharedString, size: f32) -> f32 {
    let mut font = window.text_style().font();
    font.weight = FontWeight::BOLD;
    let run = TextRun {
        len: text.len(),
        font,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(window.text_system().shape_line(text.clone(), px(size), &[run], None).width)
}

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("enter", Confirm, c),
        KeyBinding::new("space", Open, c),
        KeyBinding::new("escape", Cancel, c),
    ]);
}

pub enum SelectEvent {
    /// The user picked the option with this `value`.
    Changed(SharedString),
}

#[derive(Clone)]
pub struct SelectOption {
    pub value: SharedString,
    pub label: SharedString,
}

impl SelectOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

/// Stateful select. `value`s are strings (map your enum/number to them).
///
/// ```ignore
/// let mode = cx.new(|cx| Select::new(
///     [("0", "Windowed"), ("1", "Fullscreen"), ("2", "Fullscreen Windowed")], cx));
/// mode.update(cx, |s, cx| s.set_selected(Some("1"), cx));
/// cx.subscribe(&mode, |this, _, SelectEvent::Changed(v), cx| this.set_mode(v.parse().unwrap(), cx)).detach();
///
/// // "Choose preset" style: always shows the placeholder, only fires events
/// cx.new(|cx| Select::new(presets, cx).placeholder("Choose preset").momentary())
/// ```
pub struct Select {
    focus: FocusHandle,
    options: Vec<SelectOption>,
    selected: Option<usize>,
    highlight: usize,
    placeholder: SharedString,
    momentary: bool,
    disabled: bool,
    open: bool,
    bounds: Bounds<Pixels>,
    scroll: ScrollHandle,
    _blur: Option<Subscription>,
}

impl EventEmitter<SelectEvent> for Select {}

impl Focusable for Select {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Select {
    pub fn new<V: Into<SharedString>, L: Into<SharedString>>(
        options: impl IntoIterator<Item = (V, L)>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus: cx.focus_handle().tab_stop(true),
            options: options
                .into_iter()
                .map(|(v, l)| SelectOption::new(v, l))
                .collect(),
            selected: None,
            highlight: 0,
            placeholder: "Select...".into(),
            momentary: false,
            disabled: false,
            open: false,
            bounds: Bounds::default(),
            scroll: ScrollHandle::new(),
            _blur: None,
        }
    }

    pub fn placeholder(mut self, p: impl Into<SharedString>) -> Self {
        self.placeholder = p.into();
        self
    }

    /// Selection snaps back to "nothing" after each pick (preset pickers).
    pub fn momentary(mut self) -> Self {
        self.momentary = true;
        self
    }

    pub fn set_options<V: Into<SharedString>, L: Into<SharedString>>(
        &mut self,
        options: impl IntoIterator<Item = (V, L)>,
        cx: &mut Context<Self>,
    ) {
        let cur = self.selected_value();
        self.options = options
            .into_iter()
            .map(|(v, l)| SelectOption::new(v, l))
            .collect();
        self.selected = cur.and_then(|c| self.options.iter().position(|o| o.value == c));
        cx.notify();
    }

    pub fn selected_value(&self) -> Option<SharedString> {
        self.selected.map(|i| self.options[i].value.clone())
    }

    /// Mirror external state (no event).
    pub fn set_selected(&mut self, value: Option<&str>, cx: &mut Context<Self>) {
        let ix = value.and_then(|v| self.options.iter().position(|o| o.value == v));
        if ix != self.selected {
            self.selected = ix;
            cx.notify();
        }
    }

    #[allow(dead_code)] // kit API
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            self.open = false;
            cx.notify();
        }
    }

    fn open_popup(&mut self, cx: &mut Context<Self>) {
        if self.disabled || self.options.is_empty() {
            return;
        }
        self.open = true;
        self.highlight = self.selected.unwrap_or(0);
        self.scroll.scroll_to_item(self.highlight);
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
    }

    fn pick(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(opt) = self.options.get(ix) else {
            return;
        };
        self.selected = if self.momentary { None } else { Some(ix) };
        self.open = false;
        cx.emit(SelectEvent::Changed(opt.value.clone()));
        cx.notify();
    }

    fn mv(&mut self, delta: isize, cx: &mut Context<Self>) {
        if !self.open {
            // Closed: arrows change the value directly, like a native select.
            if self.momentary || self.disabled || self.options.is_empty() {
                return;
            }
            let cur = self
                .selected
                .map(|i| i as isize)
                .unwrap_or(if delta > 0 { -1 } else { 0 });
            let n = (cur + delta).clamp(0, self.options.len() as isize - 1) as usize;
            if Some(n) != self.selected {
                self.pick(n, cx);
            }
            return;
        }
        let n =
            (self.highlight as isize + delta).clamp(0, self.options.len() as isize - 1) as usize;
        self.highlight = n;
        self.scroll.scroll_to_item(n);
        cx.notify();
    }
}

impl Render for Select {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        if self._blur.is_none() {
            self._blur =
                Some(cx.on_focus_out(&self.focus, window, |this, _, _, cx| this.close(cx)));
        }
        let focused = self.focus.is_focused(window);
        let this = cx.entity();
        let label: SharedString = self
            .selected
            .map(|i| self.options[i].label.clone())
            .unwrap_or_else(|| self.placeholder.clone());
        let is_placeholder = self.selected.is_none();
        let list_h = px(ROW_H * self.options.len().min(MAX_VISIBLE) as f32 + 8.);
        let viewport_h = window.viewport_size().height;
        let viewport_w = window.viewport_size().width;

        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|s, _: &Up, _, cx| s.mv(-1, cx)))
            .on_action(cx.listener(|s, _: &Down, _, cx| s.mv(1, cx)))
            .on_action(cx.listener(|s, _: &Open, _, cx| {
                if s.open {
                    s.close(cx)
                } else {
                    s.open_popup(cx)
                }
            }))
            .on_action(cx.listener(|s, _: &Confirm, _, cx| {
                if s.open {
                    let h = s.highlight;
                    s.pick(h, cx);
                } else {
                    s.open_popup(cx);
                }
            }))
            .on_action(cx.listener(|s, _: &Cancel, _, cx| s.close(cx)))
            .relative()
            .w_full()
            .child(
                div()
                    .id("select-trigger")
                    .relative()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .h(px(t.control_h))
                    .px(px(12.))
                    .rounded(px(t.radius - 4.))
                    .border_1()
                    .border_color(if focused || self.open {
                        t.focus_ring
                    } else {
                        t.border
                    })
                    .bg(t.field)
                    .text_size(px(t.font_md))
                    .text_color(if is_placeholder { t.text_muted } else { t.text })
                    .map(|d| {
                        if self.disabled {
                            d.opacity(0.58).cursor(CursorStyle::OperationNotAllowed)
                        } else {
                            d.cursor_pointer()
                                .hover(move |s| s.bg(gpui::black().opacity(0.34)))
                        }
                    })
                    .child(div().flex_1().min_w_0().child(ellipsis(label)))
                    .child(
                        icon(Icon::ChevronDown)
                            .size(px(15.))
                            .text_color(t.text_muted),
                    )
                    .child(
                        canvas(
                            move |b, _, cx| this.update(cx, |s, _| s.bounds = b),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|s, _: &MouseDownEvent, window, cx| {
                            window.focus(&s.focus);
                            if s.open {
                                s.close(cx)
                            } else {
                                s.open_popup(cx)
                            }
                        }),
                    ),
            )
            .when(self.open, |d| {
                // At least as wide as the trigger, grown to the widest option (bold: the selected
                // row is bold), bounded by the window.
                let trigger_w = self.bounds.size.width;
                let widest = self.options.iter().map(|o| text_width(window, &o.label, t.font_md)).fold(0f32, f32::max);
                let want = px(widest + POPUP_CHROME);
                let width = want.max(trigger_w).min((viewport_w - px(16.)).max(trigger_w));
                // Place below the trigger, or above when there is not enough room below.
                let below = viewport_h - self.bounds.bottom() - px(12.);
                let above = self.bounds.top() - px(12.);
                let go_up = list_h > below && above > below;
                let list_h = list_h.min(if go_up { above } else { below }.max(px(72.)));
                let (pos, corner) = if go_up {
                    (
                        point(self.bounds.left(), self.bounds.top() - px(4.) - list_h),
                        Corner::TopLeft,
                    )
                } else {
                    (
                        point(self.bounds.left(), self.bounds.bottom() + px(4.)),
                        Corner::TopLeft,
                    )
                };
                d.child(
                    deferred(
                        anchored()
                            .position(pos)
                            .anchor(corner)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div()
                                    .occlude()
                                    .w(width)
                                    .min_w(trigger_w)
                                    .on_mouse_down_out(cx.listener(|s, _, _, cx| s.close(cx)))
                                    .child(
                                        div()
                                            .relative()
                                            .h(list_h)
                                            .rounded(px(t.radius - 3.))
                                            .border_1()
                                            .border_color(t.border_strong)
                                            .bg(t.modal)
                                            .shadow_lg()
                                            .child(
                                                div()
                                                    .id("select-list")
                                                    .size_full()
                                                    .p(px(3.))
                                                    .overflow_y_scroll()
                                                    .track_scroll(&self.scroll)
                                                    .children(self.options.iter().enumerate().map(
                                                        |(i, o)| {
                                                            let hl = i == self.highlight;
                                                            let sel = Some(i) == self.selected;
                                                            div()
                                                                .id(("opt", i))
                                                                .flex()
                                                                .items_center()
                                                                .justify_between()
                                                                .h(px(ROW_H))
                                                                .px(px(10.))
                                                                .rounded(px(t.radius_xs + 1.))
                                                                .text_size(px(t.font_md))
                                                                .text_color(t.text)
                                                                .cursor_pointer()
                                                                .when(sel, |d| {
                                                                    d.font_weight(FontWeight::BOLD)
                                                                })
                                                                .when(hl, |d| d.bg(t.hover_strong))
                                                                .on_mouse_move(cx.listener(
                                                                    move |s, _, _, cx| {
                                                                        if s.highlight != i {
                                                                            s.highlight = i;
                                                                            cx.notify();
                                                                        }
                                                                    },
                                                                ))
                                                                .on_click(cx.listener(
                                                                    move |s, _, _, cx| {
                                                                        s.pick(i, cx)
                                                                    },
                                                                ))
                                                                .child(
                                                                    div()
                                                                        .min_w_0()
                                                                        .child(ellipsis(o.label.clone())),
                                                                )
                                                                .when(sel, |d| {
                                                                    d.child(
                                                                        icon(Icon::Check)
                                                                            .size(px(14.))
                                                                            .text_color(t.accent),
                                                                    )
                                                                })
                                                        },
                                                    )),
                                            )
                                            .child(Scrollbar::new(
                                                "select-bar",
                                                self.scroll.clone(),
                                            )),
                                    ),
                            ),
                    )
                    .with_priority(2),
                )
            })
    }
}
