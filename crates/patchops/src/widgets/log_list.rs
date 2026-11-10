//! Virtualised activity log: colour-coded categories, follows the tail while you are at the
//! bottom (pauses when you scroll up, resumes when you return), click a row + Ctrl+C to copy it,
//! `copy_all` for a "Copy Logs" button.

use std::collections::VecDeque;

use gpui::{
    actions, div, prelude::*, px, uniform_list, App, ClipboardItem, Context, FocusHandle,
    Focusable, FontWeight, Hsla, KeyBinding, Render, ScrollStrategy, SharedString,
    UniformListScrollHandle, Window,
};

use super::scroll::Scrollbar;
use crate::theme::{ActiveTheme, Theme};

actions!(patchops_log, [CopyLine]);
const CONTEXT: &str = "PatchOpsLog";
const ROW_H: f32 = 18.0;
/// Messages longer than this (bytes) get a tooltip with the full text.
const LONG_LINE: usize = 90;

pub fn bind_keys(cx: &mut App) {
    #[cfg(target_os = "macos")]
    let m = "cmd";
    #[cfg(not(target_os = "macos"))]
    let m = "ctrl";
    cx.bind_keys([KeyBinding::new(&format!("{m}-c"), CopyLine, Some(CONTEXT))]);
}

/// Legacy log categories (`Info` / `Success` / `Warning` / `Error`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl LogLevel {
    /// Map the core's category string; unknown categories are `Info`.
    pub fn from_category(c: &str) -> Self {
        match c {
            "Success" => Self::Success,
            "Warning" => Self::Warning,
            "Error" => Self::Error,
            _ => Self::Info,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Success => "Success",
            Self::Warning => "Warning",
            Self::Error => "Error",
        }
    }
    fn color(self, t: &Theme) -> Hsla {
        match self {
            Self::Info => t.text_muted,
            Self::Success => t.ok,
            Self::Warning => t.warning,
            Self::Error => t.danger,
        }
    }
}

struct Line {
    level: LogLevel,
    message: SharedString,
}

/// Stateful log pane. Create with `cx.new(|cx| LogList::new(cx))`, feed with `push`.
///
/// ```ignore
/// let log = cx.new(|cx| LogList::new(cx));
/// log.update(cx, |l, cx| l.push(LogLevel::Success, "T7 Patch installed", cx));
/// div().h(px(180.)).child(log.clone())            // needs a bounded height
/// let text = log.read(cx).text();                 // for "Copy Logs"
/// ```
pub struct LogList {
    focus: FocusHandle,
    lines: VecDeque<Line>,
    max_lines: usize,
    scroll: UniformListScrollHandle,
    follow: bool,
    /// Line count at the last render; a change means new content to follow.
    last_len: usize,
    selected: Option<usize>,
}

impl Focusable for LogList {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl LogList {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            lines: VecDeque::new(),
            max_lines: 5000,
            scroll: UniformListScrollHandle::new(),
            follow: true,
            last_len: 0,
            selected: None,
        }
    }

    /// Ring-buffer capacity (default 5000); oldest lines are dropped first.
    pub fn max_lines(mut self, n: usize) -> Self {
        self.max_lines = n.max(1);
        self
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    #[allow(dead_code)] // kit API
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    #[allow(dead_code)] // kit API
    pub fn push(
        &mut self,
        level: LogLevel,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.push_raw(level, message.into());
        cx.notify();
    }

    /// Append many `(category, message)` pairs with a single repaint.
    pub fn extend<'a>(
        &mut self,
        items: impl IntoIterator<Item = (&'a str, SharedString)>,
        cx: &mut Context<Self>,
    ) {
        for (cat, msg) in items {
            self.push_raw(LogLevel::from_category(cat), msg);
        }
        cx.notify();
    }

    fn push_raw(&mut self, level: LogLevel, message: SharedString) {
        if self.lines.len() >= self.max_lines {
            self.lines.pop_front();
            self.selected = self.selected.and_then(|s| s.checked_sub(1));
        }
        self.lines.push_back(Line { level, message });
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.lines.clear();
        self.selected = None;
        self.follow = true;
        cx.notify();
    }

    /// All lines as `Category message` text, newline separated.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for l in &self.lines {
            s.push_str(l.level.label());
            s.push(' ');
            s.push_str(&l.message);
            s.push('\n');
        }
        s
    }

    pub fn copy_all(&self, cx: &mut App) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.text()));
    }

    fn at_bottom(&self) -> bool {
        let h = self.scroll.0.borrow().base_handle.clone();
        let max = h.max_offset().height;
        // Tolerance covers the list's own vertical padding.
        max <= px(1.) || max + h.offset().y <= px(ROW_H / 2.)
    }

    fn copy_selected(&mut self, _: &CopyLine, _: &mut Window, cx: &mut Context<Self>) {
        match self.selected.and_then(|i| self.lines.get(i)) {
            Some(l) => cx.write_to_clipboard(ClipboardItem::new_string(format!(
                "{} {}",
                l.level.label(),
                l.message
            ))),
            None => self.copy_all(cx),
        }
    }
}

impl Render for LogList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let len = self.lines.len();
        // Follow-mode bookkeeping: new content while following -> scroll to the tail; otherwise
        // being at the bottom (re)enables following and being elsewhere disables it.
        if self.last_len != len {
            self.last_len = len;
            if self.follow && len > 0 {
                self.scroll.scroll_to_item(len - 1, ScrollStrategy::Bottom);
            }
        } else if self.at_bottom() {
            self.follow = true;
        } else if self.follow {
            self.follow = false;
        }
        let focused = self.focus.is_focused(window);
        let bar_handle = self.scroll.0.borrow().base_handle.clone();

        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::copy_selected))
            .relative()
            .size_full()
            .min_h_0()
            .rounded(px(9.))
            .border_1()
            .border_color(if focused {
                t.focus_ring
            } else {
                gpui::black().opacity(0.18)
            })
            .bg(gpui::black().opacity(0.34))
            .font_family(t.font_mono)
            .text_size(px(t.font_xs))
            .child(if len == 0 {
                div()
                    .px(px(10.))
                    .py(px(8.))
                    .text_color(t.text_muted)
                    .child("No activity yet.")
                    .into_any_element()
            } else {
                uniform_list(
                    "log-rows",
                    len,
                    cx.processor(move |this, range: std::ops::Range<usize>, _w, cx| {
                        let t = *cx.theme();
                        range
                            .filter_map(|ix| this.lines.get(ix).map(|l| (ix, l)))
                            .map(|(ix, l)| {
                                let sel = this.selected == Some(ix);
                                div()
                                    .id(ix)
                                    .flex()
                                    .gap(px(10.))
                                    .h(px(ROW_H))
                                    .px(px(10.))
                                    .items_center()
                                    .when(sel, |d| d.bg(t.hover_strong))
                                    .text_color(t.text.opacity(0.76))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        window.focus(&this.focus);
                                        this.selected = if this.selected == Some(ix) {
                                            None
                                        } else {
                                            Some(ix)
                                        };
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .w(px(64.))
                                            .flex_none()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(l.level.color(&t))
                                            .child(l.level.label()),
                                    )
                                    .child(
                                        div()
                                            .id(("msg", ix))
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            // Rows are fixed-height (virtualised) so a long line is
                                            // cut with an ellipsis; the tooltip has the whole line.
                                            .when(l.message.len() > LONG_LINE, |d| {
                                                d.tooltip(super::tooltip::tooltip(l.message.clone()))
                                            })
                                            .child(l.message.clone()),
                                    )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.scroll.clone())
                .py(px(4.))
                .size_full()
                .into_any_element()
            })
            .child(Scrollbar::new("log-bar", bar_handle))
    }
}
