use std::sync::Arc;

use gpui::{
    div, prelude::*, px, uniform_list, Context, Render, ScrollStrategy, UniformListScrollHandle,
    Window,
};

use super::{heading, page};
use crate::state::SharedState;
use crate::theme::ActiveTheme;
use crate::widgets::Button;

/// PLACEHOLDER: virtualised log pane (`uniform_list`) over `SharedState::log`.
pub struct LogsView {
    shared: Arc<SharedState>,
    scroll: UniformListScrollHandle,
}

impl LogsView {
    pub fn new(shared: Arc<SharedState>, _cx: &mut Context<Self>) -> Self {
        Self { shared, scroll: UniformListScrollHandle::new() }
    }
}

impl Render for LogsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let len = self.shared.log_len();
        let mono = t.font_mono;
        let (text, muted) = (t.text, t.text_muted);

        page(&t)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(heading(&t, format!("Logs ({len} lines)")))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(Button::new("log-top", "Top").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                                    cx.notify();
                                },
                            )))
                            .child(Button::new("log-bottom", "Bottom").on_click(cx.listener(
                                |this, _, _, cx| {
                                    let n = this.shared.log_len();
                                    this.scroll.scroll_to_item(n.saturating_sub(1), ScrollStrategy::Bottom);
                                    cx.notify();
                                },
                            )))
                            .child(Button::new("log-add", "Add 100").primary().on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.shared.with(|d| {
                                        let n = d.log.len();
                                        d.log.extend((n..n + 100).map(|i| format!("[{i:05}] appended line")));
                                    });
                                    cx.notify();
                                },
                            ))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.sidebar)
                    .child(
                        uniform_list(
                            "log-lines",
                            len,
                            cx.processor(move |this, range: std::ops::Range<usize>, _w, _cx| {
                                let data = this.shared.snapshot_range(range.clone());
                                range
                                    .zip(data)
                                    .map(|(ix, line)| {
                                        div()
                                            .id(ix)
                                            .px_2()
                                            .h(px(18.))
                                            .font_family(mono)
                                            .text_xs()
                                            .text_color(if ix % 2 == 0 { text } else { muted })
                                            .child(line)
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(self.scroll.clone())
                        .size_full(),
                    ),
            )
    }
}
