use std::sync::Arc;

use gpui::{div, prelude::*, px, Context, Entity, Render, Window};

use super::{heading, page, row};
use crate::state::SharedState;
use crate::theme::ActiveTheme;
use crate::widgets::{modal::modal, Button, Slider, SliderEvent, TextInput, TextInputEvent, Toggle};

/// PLACEHOLDER: exercises button, toggle, slider, text input and modal.
pub struct DashboardView {
    shared: Arc<SharedState>,
    slider: Entity<Slider>,
    input: Entity<TextInput>,
    modal_open: bool,
}

impl DashboardView {
    pub fn new(shared: Arc<SharedState>, cx: &mut Context<Self>) -> Self {
        let init = shared.with(|d| d.slider);
        let slider = cx.new(|_| Slider::new(0.0, 1.0, init));
        cx.subscribe(&slider, |this, _, ev: &SliderEvent, cx| {
            let SliderEvent::Changed(v) = ev;
            this.shared.with(|d| d.slider = *v);
            cx.notify();
        })
        .detach();

        let input = cx.new(|cx| TextInput::new("Type something...", cx));
        cx.subscribe(&input, |this, _, ev: &TextInputEvent, cx| {
            match ev {
                TextInputEvent::Changed(s) => this.shared.with(|d| d.text = s.to_string()),
                TextInputEvent::Submit(s) => this.shared.push_log(format!("submitted: {s}")),
            }
            cx.notify();
        })
        .detach();

        Self { shared, slider, input, modal_open: false }
    }
}

impl Render for DashboardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let data = self.shared.snapshot();
        // Pick up changes made elsewhere (e.g. via RPC) without emitting events.
        self.slider.update(cx, |s, _| {
            if (s.value() - data.slider).abs() > f32::EPSILON {
                s.set_value_silent(data.slider);
            }
        });

        // Handlers get `&mut App`, not `&mut Context<Self>`. Use `cx.listener(..)` when the
        // callback type allows, or capture `cx.entity()` and `.update()` it otherwise.
        let click = Button::new("inc", format!("Clicked {} times", data.counter))
            .primary()
            .on_click(cx.listener(|this, _, _, cx| {
                this.shared.with(|d| d.counter += 1);
                cx.notify();
            }));

        let this = cx.entity();
        let toggle = Toggle::new("toggle", data.toggle).on_change(move |v, _, cx| {
            this.update(cx, |this, cx| {
                this.shared.with(|d| d.toggle = v);
                cx.notify();
            })
        });

        page(&t)
            .child(heading(&t, "Dashboard"))
            .child(row(&t, "Button", click))
            .child(row(&t, "Toggle", div().flex().gap_3().items_center().child(toggle).child(if data.toggle { "on" } else { "off" })))
            .child(row(
                &t,
                "Slider",
                div().flex().gap_3().items_center().child(self.slider.clone()).child(format!("{:.0}%", data.slider * 100.0)),
            ))
            .child(row(&t, "Text input", div().w(px(320.)).child(self.input.clone())))
            .child(row(&t, "You typed", div().child(format!("{:?}", data.text))))
            .child(row(
                &t,
                "Modal",
                Button::new("open-modal", "Open modal").on_click(cx.listener(|this, _, _, cx| {
                    this.modal_open = true;
                    cx.notify();
                })),
            ))
            .when(self.modal_open, |d| {
                let this = cx.entity();
                d.child(modal(
                    "Confirm",
                    "This is an in-window modal dialog.",
                    move |_, cx| {
                        this.update(cx, |v, cx| {
                            v.modal_open = false;
                            cx.notify();
                        })
                    },
                    cx,
                ))
            })
    }
}
