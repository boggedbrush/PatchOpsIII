use std::sync::Arc;
use std::time::Duration;

use gpui::{div, prelude::*, Context, Render, Window};

use super::{heading, page};
use crate::state::SharedState;
use crate::theme::ActiveTheme;
use crate::widgets::Button;

/// PLACEHOLDER: shows the "run work off the UI thread, then update the view" pattern.
pub struct TasksView {
    shared: Arc<SharedState>,
    running: bool,
    results: Vec<String>,
}

impl TasksView {
    pub fn new(shared: Arc<SharedState>, _cx: &mut Context<Self>) -> Self {
        Self { shared, running: false, results: Vec::new() }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        self.running = true;
        cx.notify();
        // `this` is a WeakEntity<Self>; `cx` an AsyncApp usable across awaits.
        cx.spawn(async move |this, cx| {
            // Heavy / blocking work goes to the background thread pool.
            let sum = cx
                .background_executor()
                .spawn(async {
                    std::thread::sleep(Duration::from_millis(1500));
                    (1..=1_000_000u64).sum::<u64>()
                })
                .await;
            // Back on the UI thread: mutate the view and re-render.
            this.update(cx, |this, cx| {
                this.running = false;
                let msg = format!("background task finished: sum = {sum}");
                this.shared.push_log(msg.clone());
                this.results.push(msg);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for TasksView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        page(&t)
            .child(heading(&t, "Tasks"))
            .child(
                div().flex().items_center().gap_3().child(
                    Button::new("run-task", if self.running { "Running..." } else { "Run background task" })
                        .primary()
                        .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
                ),
            )
            .children(self.results.iter().rev().map(|r| div().text_color(t.text_muted).child(r.clone())))
    }
}
