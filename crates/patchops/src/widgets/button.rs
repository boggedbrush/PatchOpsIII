use gpui::{
    div, prelude::*, px, App, ClickEvent, ElementId, InteractiveElement, IntoElement, SharedString,
    Window,
};

use crate::theme::ActiveTheme;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    primary: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            primary: false,
            on_click: None,
        }
    }

    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme().clone();
        let (bg, hover) = if self.primary {
            (t.accent, t.accent_hover)
        } else {
            (t.panel, t.hover)
        };
        div()
            .id(self.id)
            .px_3()
            .py_1()
            .rounded(px(6.))
            .border_1()
            .border_color(t.border)
            .bg(bg)
            .text_color(t.text)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(self.label)
            .when_some(self.on_click, |d, f| d.on_click(f))
    }
}
