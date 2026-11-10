//! Spinner and progress bar.

use std::time::Duration;

use gpui::{
    div, percentage, prelude::*, px, relative, Animation, AnimationExt as _, App, ElementId, Hsla,
    IntoElement, Pixels, Transformation, Window,
};

use super::icon::{icon, Icon};
use crate::theme::ActiveTheme;

/// Rotating `loader-circle` icon. Only animates (and only requests frames) while on screen.
///
/// ```ignore
/// Spinner::new("busy").size(px(16.))
/// ```
#[derive(IntoElement)]
pub struct Spinner {
    id: ElementId,
    size: Pixels,
    color: Option<Hsla>,
}

impl Spinner {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            size: px(16.),
            color: None,
        }
    }
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(cx.theme().text);
        icon(Icon::LoaderCircle)
            .size(self.size)
            .text_color(color)
            .with_animation(
                self.id,
                Animation::new(Duration::from_millis(900)).repeat(),
                |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
            )
    }
}

/// Thin progress bar. `value: Some(0.0..=1.0)` is determinate; `None` is the indeterminate
/// scanning bar used on the legacy startup screen.
///
/// ```ignore
/// ProgressBar::new("dl").value(Some(0.4)).width(px(200.))
/// ```
#[derive(IntoElement)]
pub struct ProgressBar {
    id: ElementId,
    value: Option<f32>,
    width: Option<Pixels>,
}

impl ProgressBar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            value: None,
            width: None,
        }
    }
    pub fn value(mut self, value: Option<f32>) -> Self {
        self.value = value.map(|v| v.clamp(0., 1.));
        self
    }
    /// Fixed width; default is full width of the parent.
    pub fn width(mut self, w: Pixels) -> Self {
        self.width = Some(w);
        self
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let track = div()
            .relative()
            .h(px(3.))
            .rounded_full()
            .overflow_hidden()
            .bg(t.hover_strong);
        let track = match self.width {
            Some(w) => track.w(w),
            None => track.w_full(),
        };
        match self.value {
            Some(v) => track
                .child(div().h_full().w(relative(v)).rounded_full().bg(t.accent))
                .into_any_element(),
            None => track
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .w(relative(0.42))
                        .rounded_full()
                        .bg(t.accent)
                        .with_animation(
                            self.id,
                            Animation::new(Duration::from_millis(1450)).repeat(),
                            |d, delta| d.left(relative(-0.42 + delta * 1.42)),
                        ),
                )
                .into_any_element(),
        }
    }
}
