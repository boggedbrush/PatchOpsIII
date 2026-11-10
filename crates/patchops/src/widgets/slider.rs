use gpui::{
    canvas, div, prelude::*, px, Bounds, Context, EventEmitter, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Render, Window,
};

use crate::theme::ActiveTheme;

pub enum SliderEvent {
    /// Emitted continuously while dragging.
    Changed(f32),
}

/// Horizontal slider over `min..=max`. Stateful: create with `cx.new(|_| Slider::new(..))`
/// and subscribe to `SliderEvent`.
pub struct Slider {
    min: f32,
    max: f32,
    value: f32,
    bounds: Bounds<Pixels>,
    dragging: bool,
}

impl EventEmitter<SliderEvent> for Slider {}

impl Slider {
    pub fn new(min: f32, max: f32, value: f32) -> Self {
        Self {
            min,
            max,
            value,
            bounds: Bounds::default(),
            dragging: false,
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// Set without emitting an event (use when syncing from external state).
    pub fn set_value_silent(&mut self, v: f32) {
        self.value = v.clamp(self.min, self.max);
    }

    fn set_from_x(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let w = f32::from(self.bounds.size.width).max(1.0);
        let frac = (f32::from(x - self.bounds.left()) / w).clamp(0.0, 1.0);
        self.value = self.min + frac * (self.max - self.min);
        cx.emit(SliderEvent::Changed(self.value));
        cx.notify();
    }
}

impl Render for Slider {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let frac = (self.value - self.min) / (self.max - self.min).max(f32::EPSILON);
        let this = cx.entity();
        div()
            .relative()
            .w(px(240.))
            .h(px(22.))
            .flex()
            .items_center()
            .cursor_pointer()
            // Zero-size canvas stretched over the element: records its laid-out bounds.
            .child(
                canvas(
                    move |bounds, _, cx| this.update(cx, |s, _| s.bounds = bounds),
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
                    .bg(t.border)
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
                            .top(px(-7.))
                            .left(gpui::relative(frac))
                            .ml(px(-9.))
                            .size(px(18.))
                            .rounded_full()
                            .bg(t.text)
                            .border_2()
                            .border_color(t.accent),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, e: &MouseDownEvent, _, cx| {
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
                cx.listener(|s, _: &MouseUpEvent, _, _| s.dragging = false),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|s, _: &MouseUpEvent, _, _| s.dragging = false),
            )
    }
}
