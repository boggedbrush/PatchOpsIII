use gpui::{
    deferred, div, prelude::*, px, AnyElement, App, IntoElement, MouseButton, SharedString, Window,
};

use crate::theme::ActiveTheme;
use crate::widgets::Button;

/// In-window modal dialog: dimmed backdrop + centred card. Render it as the *last*
/// child of the root so it paints on top, and only while the owner's `open` flag is set.
/// Clicking the backdrop or the Close button calls `on_close`.
pub fn modal(
    title: impl Into<SharedString>,
    body: impl IntoElement,
    on_close: impl Fn(&mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> AnyElement {
    let t = cx.theme().clone();
    let close_bg = on_close.clone();
    let close_btn = on_close;
    deferred(
        div()
            .id("modal-backdrop")
            .absolute()
            .inset_0()
            .occlude() // swallow mouse events so nothing underneath reacts
            .bg(gpui::black().opacity(0.55))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, move |_, w, cx| close_bg(w, cx))
            .child(
                div()
                    .id("modal-card")
                    .w(px(420.))
                    .p_4()
                    .gap_3()
                    .flex()
                    .flex_col()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.panel)
                    .text_color(t.text)
                    // Don't let clicks inside the card reach the backdrop handler.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(div().text_lg().child(title.into()))
                    .child(body)
                    .child(
                        div().flex().justify_end().child(
                            Button::new("modal-close", "Close")
                                .primary()
                                .on_click(move |_, w, cx| close_btn(w, cx)),
                        ),
                    ),
            ),
    )
    .with_priority(10)
    .into_any_element()
}
