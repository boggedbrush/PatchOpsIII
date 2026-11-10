//! Tooltip (legacy uses native `title=`). Attach to any stateful element:
//!
//! ```ignore
//! div().id("x").tooltip(tooltip("Hide password"))
//! ```

use gpui::{div, prelude::*, px, AnyView, App, IntoElement, Render, SharedString, Window};

use crate::theme::ActiveTheme;

struct TooltipView(SharedString);

impl Render for TooltipView {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        div()
            .max_w(px(320.))
            .px(px(9.))
            .py(px(5.))
            .rounded(px(6.))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.modal)
            .text_color(t.text)
            .text_size(px(t.font_xs))
            .child(self.0.clone())
    }
}

/// Builder closure for `.tooltip(..)`.
pub fn tooltip(
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text: SharedString = text.into();
    move |_, cx| cx.new(|_| TooltipView(text.clone())).into()
}
