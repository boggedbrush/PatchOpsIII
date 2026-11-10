//! Toast notifications: an entity that stacks transient messages bottom-right and auto-dismisses.

use std::time::Duration;

use gpui::{
    deferred, div, prelude::*, px, Context, Hsla, InteractiveElement, IntoElement, Render,
    SharedString, Task, Window,
};

use super::icon::{icon, Icon};
use crate::theme::{ActiveTheme, Theme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

impl ToastKind {
    fn parts(self, t: &Theme) -> (Icon, Hsla) {
        match self {
            Self::Info => (Icon::Info, t.text_muted),
            Self::Success => (Icon::CircleCheck, t.ok),
            Self::Warning => (Icon::TriangleAlert, t.warning),
            Self::Error => (Icon::CircleX, t.danger),
        }
    }
}

struct Toast {
    id: u64,
    kind: ToastKind,
    message: SharedString,
    /// Dropping the task cancels the pending auto-dismiss (no leaked timers).
    _timer: Task<()>,
}

/// Create once, add as the last child of the root, push from anywhere with the entity handle.
///
/// ```ignore
/// let toasts = cx.new(|_| ToastHost::new());
/// root.child(toasts.clone());
/// toasts.update(cx, |t, cx| t.success("Gamertag updated", cx));
/// toasts.update(cx, |t, cx| t.error("Install failed", cx));   // errors stay 8 s, others 4 s
/// ```
pub struct ToastHost {
    toasts: Vec<Toast>,
    next_id: u64,
    max: usize,
}

impl Default for ToastHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ToastHost {
    pub fn new() -> Self {
        Self {
            toasts: Vec::new(),
            next_id: 1,
            max: 4,
        }
    }

    pub fn push(
        &mut self,
        kind: ToastKind,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_id;
        self.next_id += 1;
        let secs = if kind == ToastKind::Error { 8 } else { 4 };
        let timer = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(secs))
                .await;
            this.update(cx, |t, cx| t.dismiss(id, cx)).ok();
        });
        self.toasts.push(Toast {
            id,
            kind,
            message: message.into(),
            _timer: timer,
        });
        if self.toasts.len() > self.max {
            self.toasts.remove(0);
        }
        cx.notify();
    }

    pub fn info(&mut self, m: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push(ToastKind::Info, m, cx);
    }
    pub fn success(&mut self, m: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push(ToastKind::Success, m, cx);
    }
    pub fn warning(&mut self, m: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push(ToastKind::Warning, m, cx);
    }
    pub fn error(&mut self, m: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push(ToastKind::Error, m, cx);
    }

    pub fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.toasts.retain(|t| t.id != id);
        cx.notify();
    }
}

impl Render for ToastHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        if self.toasts.is_empty() {
            return div().into_any_element();
        }
        deferred(
            div()
                .absolute()
                .right(px(16.))
                .bottom(px(16.))
                .flex()
                .flex_col()
                .items_end()
                .gap(px(8.))
                .children(self.toasts.iter().map(|toast| {
                    let (ic, color) = toast.kind.parts(&t);
                    let id = toast.id;
                    div()
                        .id(("toast", id))
                        .occlude()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .w(px(340.))
                        .px(px(12.))
                        .py(px(10.))
                        .rounded(px(10.))
                        .border_1()
                        .border_color(color.opacity(0.45))
                        .bg(t.modal)
                        .shadow_lg()
                        .text_size(px(t.font_sm))
                        .text_color(t.text)
                        .child(icon(ic).size(px(18.)).text_color(color))
                        .child(div().flex_1().min_w_0().child(toast.message.clone()))
                        .child(
                            div()
                                .id(("toast-x", id))
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(22.))
                                .rounded(px(5.))
                                .cursor_pointer()
                                .hover(move |s| s.bg(t.hover_strong))
                                .child(icon(Icon::X).size(px(14.)).text_color(t.text_muted))
                                .on_click(cx.listener(move |this, _, _, cx| this.dismiss(id, cx))),
                        )
                }))
                .into_any_element(),
        )
        .with_priority(20)
        .into_any_element()
    }
}
