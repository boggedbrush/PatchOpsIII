use gpui::{
    div, prelude::*, px, App, ElementId, InteractiveElement, IntoElement, MouseButton, Window,
};

use crate::theme::ActiveTheme;

type ChangeHandler = Box<dyn Fn(bool, &mut Window, &mut App) + 'static>;

/// Stateless on/off switch: the owner keeps `checked` and flips it in `on_change`.
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    checked: bool,
    on_change: Option<ChangeHandler>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            on_change: None,
        }
    }

    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme().clone();
        let checked = self.checked;
        div()
            .id(self.id)
            .w(px(40.))
            .h(px(22.))
            .rounded_full()
            .p(px(2.))
            .flex()
            .items_center()
            .bg(if checked { t.accent } else { t.border })
            .when(checked, |d| d.justify_end())
            .cursor_pointer()
            .child(div().size(px(18.)).rounded_full().bg(t.text))
            .when_some(self.on_change, move |d, f| {
                d.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    f(!checked, window, cx)
                })
            })
    }
}
