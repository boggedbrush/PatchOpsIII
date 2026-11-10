//! One file per sidebar tab. Everything here is PLACEHOLDER demo content.

mod about;
mod dashboard;
pub mod gallery;
mod logs;
mod tasks;

pub use about::AboutView;
pub use dashboard::DashboardView;
pub use gallery::GalleryView;
pub use logs::LogsView;
pub use tasks::TasksView;

use gpui::{div, prelude::*, px, Div, SharedString};

use crate::theme::Theme;

/// Shared page chrome helpers.
pub fn page(t: &Theme) -> Div {
    div().size_full().p_5().flex().flex_col().gap_4().bg(t.bg)
}

pub fn heading(t: &Theme, text: impl Into<SharedString>) -> Div {
    div().text_xl().text_color(t.text).child(text.into())
}

pub fn row(t: &Theme, label: &str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap_4()
        .child(div().w(px(140.)).text_color(t.text_muted).child(label.to_string()))
        .child(control)
}
