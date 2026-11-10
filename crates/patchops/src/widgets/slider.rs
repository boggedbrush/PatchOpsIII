//! Horizontal slider with value label (legacy `.slider-row`: red fill, number at the right).

use gpui::{
    actions, canvas, div, prelude::*, px, App, Bounds, Context, CursorStyle, EventEmitter,
    FocusHandle, Focusable, FontWeight, IntoElement, KeyBinding, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Render, SharedString, Window,
};

use crate::theme::ActiveTheme;

actions!(
    patchops_slider,
    [StepDown, StepUp, PageDown, PageUp, ToMin, ToMax]
);
const CONTEXT: &str = "PatchOpsSlider";

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("left", StepDown, c),
        KeyBinding::new("down", StepDown, c),
        KeyBinding::new("right", StepUp, c),
        KeyBinding::new("up", StepUp, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("home", ToMin, c),
        KeyBinding::new("end", ToMax, c),
    ]);
}

pub enum SliderEvent {
    /// Continuously while dragging / on every key step.
    Changed(f32),
    /// Drag released or key pressed: the value to persist (legacy commits on mouse-up).
    Committed(f32),
}

/// Stateful slider over `min..=max`; drag, click the track, or use the arrow keys / PageUp /
/// PageDown / Home / End while focused.
///
/// ```ignore
/// let fov = cx.new(|cx| Slider::new(65., 120., 80., cx).step(1.).label_width(px(44.)));
/// cx.subscribe(&fov, |this, _, ev: &SliderEvent, cx| {
///     if let SliderEvent::Committed(v) = ev { this.set_fov(*v as i32, cx) }
/// }).detach();
/// fov.update(cx, |s, _| s.set_value_silent(90.));   // mirror external state
/// ```
pub struct Slider {
    focus_handle: FocusHandle,
    min: f32,
    max: f32,
    step: f32,
    value: f32,
    show_label: bool,
    label_w: Pixels,
    disabled: bool,
    bounds: Bounds<Pixels>,
    dragging: bool,
}

impl EventEmitter<SliderEvent> for Slider {}

impl Focusable for Slider {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Slider {
    pub fn new(min: f32, max: f32, value: f32, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            min,
            max,
            step: 1.0,
            value: value.clamp(min, max),
            show_label: true,
            label_w: px(44.),
            disabled: false,
            bounds: Bounds::default(),
            dragging: false,
        }
    }

    /// Snap increment (default 1). Key steps move by `step`, PageUp/Down by 10 steps.
    pub fn step(mut self, step: f32) -> Self {
        self.step = step.max(f32::EPSILON);
        self
    }

    /// Width of the numeric label at the right (default 44px).
    pub fn label_width(mut self, w: Pixels) -> Self {
        self.label_w = w;
        self
    }

    /// Hide the numeric label.
    pub fn no_label(mut self) -> Self {
        self.show_label = false;
        self
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// Set without emitting an event (use when syncing from external state).
    pub fn set_value_silent(&mut self, v: f32) {
        if !self.dragging {
            self.value = self.snap(v);
        }
    }

    pub fn set_range(&mut self, min: f32, max: f32) {
        self.min = min;
        self.max = max;
        self.value = self.value.clamp(min, max);
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    fn snap(&self, v: f32) -> f32 {
        let steps = ((v - self.min) / self.step).round();
        (self.min + steps * self.step).clamp(self.min, self.max)
    }

    fn set_from_x(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let w = f32::from(self.bounds.size.width).max(1.0);
        let frac = (f32::from(x - self.bounds.left()) / w).clamp(0.0, 1.0);
        self.set(self.min + frac * (self.max - self.min), cx);
    }

    fn set(&mut self, v: f32, cx: &mut Context<Self>) {
        let v = self.snap(v);
        if (v - self.value).abs() > f32::EPSILON {
            self.value = v;
            cx.emit(SliderEvent::Changed(v));
            cx.notify();
        }
    }

    fn key_step(&mut self, delta: f32, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.set(self.value + delta, cx);
        cx.emit(SliderEvent::Committed(self.value));
    }

    fn label(&self) -> SharedString {
        if self.step.fract() == 0.0 {
            format!("{}", self.value as i64).into()
        } else {
            format!("{:.2}", self.value).into()
        }
    }
}

impl Render for Slider {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let frac = (self.value - self.min) / (self.max - self.min).max(f32::EPSILON);
        let this = cx.entity();
        let focused = self.focus_handle.is_focused(window);
        let page = self.step * 10.;
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(move |s, _: &StepDown, _, cx| s.key_step(-s.step, cx)))
            .on_action(cx.listener(move |s, _: &StepUp, _, cx| s.key_step(s.step, cx)))
            .on_action(cx.listener(move |s, _: &PageDown, _, cx| s.key_step(-page, cx)))
            .on_action(cx.listener(move |s, _: &PageUp, _, cx| s.key_step(page, cx)))
            .on_action(cx.listener(|s, _: &ToMin, _, cx| s.key_step(s.min - s.value, cx)))
            .on_action(cx.listener(|s, _: &ToMax, _, cx| s.key_step(s.max - s.value, cx)))
            .flex()
            .items_center()
            .gap(px(12.))
            .w_full()
            .h(px(28.))
            .when(self.disabled, |d| d.opacity(0.55))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w(px(100.))
                    .h_full()
                    .flex()
                    .items_center()
                    .cursor(if self.disabled {
                        CursorStyle::OperationNotAllowed
                    } else {
                        CursorStyle::PointingHand
                    })
                    // Zero-size canvas stretched over the element: records its laid-out bounds.
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                this.update(cx, |s, _| s.bounds = bounds);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(4.))
                            .rounded_full()
                            .bg(t.hover_strong)
                            .child(
                                div()
                                    .absolute()
                                    .h_full()
                                    .w(gpui::relative(frac))
                                    .rounded_full()
                                    .bg(t.accent),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-6.))
                                    .left(gpui::relative(frac))
                                    .ml(px(-8.))
                                    .size(px(16.))
                                    .rounded_full()
                                    .bg(t.text)
                                    .border_2()
                                    .border_color(if focused { t.focus_ring } else { t.accent }),
                            ),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|s, e: &MouseDownEvent, window, cx| {
                            if s.disabled {
                                return;
                            }
                            window.focus(&s.focus_handle);
                            s.dragging = true;
                            s.set_from_x(e.position.x, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(|s, e: &MouseMoveEvent, _, cx| {
                        if s.dragging && e.dragging() {
                            s.set_from_x(e.position.x, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|s, _: &MouseUpEvent, _, cx| {
                            if s.dragging {
                                s.dragging = false;
                                cx.emit(SliderEvent::Committed(s.value));
                            }
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|s, _: &MouseUpEvent, _, cx| {
                            if s.dragging {
                                s.dragging = false;
                                cx.emit(SliderEvent::Committed(s.value));
                            }
                        }),
                    ),
            )
            .when(self.show_label, |d| {
                d.child(
                    div()
                        .w(self.label_w)
                        .flex_none()
                        .text_right()
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(t.font_md))
                        .text_color(t.text)
                        .child(self.label()),
                )
            })
    }
}
