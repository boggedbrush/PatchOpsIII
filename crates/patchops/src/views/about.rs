use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Render, TitlebarOptions, Window,
    WindowBounds, WindowOptions,
};

use super::{heading, page};
use crate::theme::ActiveTheme;
use crate::widgets::Button;

/// PLACEHOLDER: about page + example of opening a second OS window.
pub struct AboutView;

struct SecondWindow;

impl Render for SecondWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        page(&t).child(heading(&t, "Second window")).child("Separate OS window, same entity world.")
    }
}

fn open_second_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(420.), px(220.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("Second window".into()), ..Default::default() }),
            ..Default::default()
        },
        |_, cx| cx.new(|_| SecondWindow),
    )
    .ok();
}

impl Render for AboutView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        page(&t)
            .child(heading(&t, "About"))
            .child(format!("patchops-core {}", patchops_core::version()))
            .child(div().child(
                Button::new("second-window", "Open second window").on_click(|_, _, cx| open_second_window(cx)),
            ))
    }
}
