//! Sidebar navigation entry (legacy `.nav-button`).

use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, App, ClickEvent, ElementId,
    FontWeight, InteractiveElement, IntoElement, SharedString, Window,
};

use super::ellipsis;
use super::icon::{icon, Icon};
use crate::theme::ActiveTheme;

/// Icon + label row; the active item gets the red-tinted gradient.
///
/// ```ignore
/// NavItem::new("nav-graphics", Icon::Image, "Graphics").active(nav == Nav::Graphics)
///     .on_click(cx.listener(|this, _, _, cx| this.go(Nav::Graphics, cx)))
/// ```
#[derive(IntoElement)]
pub struct NavItem {
    id: ElementId,
    icon: Icon,
    label: SharedString,
    active: bool,
    on_click: Option<super::ClickFn>,
}

impl NavItem {
    pub fn new(id: impl Into<ElementId>, icon: Icon, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon,
            label: label.into(),
            active: false,
            on_click: None,
        }
    }
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for NavItem {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let active = self.active;
        div()
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(11.))
            .w_full()
            .h(px(t.row_h))
            .px(px(12.))
            .rounded(px(t.radius_sm))
            .border_1()
            .border_color(if active {
                t.accent.opacity(0.22)
            } else {
                gpui::transparent_black()
            })
            .text_color(if active { t.text } else { t.text_strong })
            .text_size(px(t.font_md))
            .font_weight(FontWeight::BOLD)
            .tab_index(0)
            .focus(move |s| s.border_color(t.focus_ring))
            .cursor_pointer()
            .map(|d| {
                if active {
                    d.bg(linear_gradient(
                        135.,
                        linear_color_stop(t.accent.opacity(0.22), 0.),
                        linear_color_stop(gpui::white().opacity(0.07), 1.),
                    ))
                } else {
                    d.hover(move |s| s.bg(t.hover).text_color(t.text))
                }
            })
            .child(icon(self.icon).size(px(18.)).text_color(if active {
                t.text
            } else {
                t.text_strong
            }))
            .child(div().min_w_0().child(ellipsis(self.label)))
            .when_some(self.on_click, |d, f| d.on_click(f))
    }
}
