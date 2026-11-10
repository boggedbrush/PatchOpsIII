//! Modal dialogs: `Dialog` (builder) and `confirm_dialog`.
//!
//! Render a dialog as the *last* child of a `relative()` root while the owner's `open` flag is
//! set. Esc and a click on the backdrop call `on_close`; the dialog grabs focus when shown.

use gpui::{
    deferred, div, prelude::*, px, AnyElement, App, ElementId, FocusHandle, FontWeight,
    InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString, Window,
};

use super::button::{Button, ButtonKind};
use super::icon::{icon, Icon};
use super::{FocusNext, FocusPrev};
use crate::theme::ActiveTheme;

type CloseHandler = std::rc::Rc<dyn Fn(&mut Window, &mut App)>;

/// Dimmed backdrop + centred card with title, description, body and a footer row.
///
/// ```ignore
/// Dialog::new("depot")
///     .title("Copy this")
///     .description("Run this command in the Steam console.")
///     .width(px(480.))
///     .child(code_block)
///     .footer(Button::new("cancel", "Cancel").on_click(..))
///     .footer(Button::new("go", "Continue").primary().on_click(..))
///     .on_close(move |_w, cx| this.update(cx, |t, cx| { t.depot = None; cx.notify() }))
/// ```
#[derive(IntoElement)]
pub struct Dialog {
    id: ElementId,
    title: Option<SharedString>,
    description: Option<SharedString>,
    width: gpui::Pixels,
    show_close: bool,
    on_close: Option<CloseHandler>,
    backdrop_closes: bool,
    initial_focus: usize,
    children: Vec<AnyElement>,
    footer: Vec<AnyElement>,
}

impl Dialog {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            title: None,
            description: None,
            width: px(460.),
            show_close: true,
            on_close: None,
            backdrop_closes: true,
            initial_focus: 0,
            children: Vec::new(),
            footer: Vec::new(),
        }
    }
    pub fn title(mut self, t: impl Into<SharedString>) -> Self {
        self.title = Some(t.into());
        self
    }
    pub fn description(mut self, d: impl Into<SharedString>) -> Self {
        self.description = Some(d.into());
        self
    }
    pub fn width(mut self, w: gpui::Pixels) -> Self {
        self.width = w;
        self
    }
    /// Hide the X button (modal that must be answered through its footer).
    pub fn no_close_button(mut self) -> Self {
        self.show_close = false;
        self
    }
    /// Called for Esc, backdrop click and the X button.
    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(std::rc::Rc::new(f));
        self
    }
    /// A click on the dimmed backdrop calls `on_close` (default). Pass `false` for prompts that
    /// must be answered through their buttons or Esc.
    pub fn backdrop_closes(mut self, yes: bool) -> Self {
        self.backdrop_closes = yes;
        self
    }
    /// Which tab stop inside the dialog gets the keyboard focus when it opens (0 = the first
    /// focusable control in tree order, the X button when there is one; the footer buttons follow).
    pub fn initial_focus(mut self, ix: usize) -> Self {
        self.initial_focus = ix;
        self
    }
    /// Add a footer element (usually a `Button`), right aligned in call order.
    pub fn footer(mut self, el: impl IntoElement) -> Self {
        self.footer.push(el.into_any_element());
        self
    }
}

impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Dialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.theme();
        let state = window.use_keyed_state("dialog-state", cx, |_, cx| DialogState {
            focus: cx.focus_handle(),
            placed: false,
        });
        let focus = state.read(cx).focus.clone();
        grab_focus(&focus, window, cx);
        if !state.read(cx).placed {
            state.update(cx, |s, _| s.placed = true);
            // Tab stops are only known once the dialog has been laid out: move the focus from the
            // card to the requested control on the next frame.
            let (card, n) = (focus.clone(), self.initial_focus);
            window.on_next_frame(move |w, cx| {
                if card.is_focused(w) {
                    for _ in 0..=n {
                        trap_tab(&card, true, w, cx);
                    }
                }
            });
        }
        let on_close = self.on_close;
        let bg_close = on_close.clone().filter(|_| self.backdrop_closes);
        let key_close = on_close.clone();
        let x_close = on_close;
        let show_x = self.show_close;

        deferred(
            div()
                .id(self.id.clone())
                .absolute()
                .inset_0()
                .occlude()
                .bg(t.overlay)
                .flex()
                .items_center()
                .justify_center()
                .p(px(20.))
                .when_some(bg_close, |d, f| {
                    d.on_mouse_down(MouseButton::Left, move |_, w, cx| f(w, cx))
                })
                .child(
                    div()
                        .id("dialog-card")
                        .track_focus(&focus)
                        .w(self.width)
                        .max_w_full()
                        .max_h_full()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .p(px(18.))
                        .rounded(px(16.))
                        .border_1()
                        .border_color(t.border_strong)
                        .bg(t.modal)
                        .shadow_lg()
                        .text_color(t.text)
                        .text_size(px(t.font_md))
                        // Clicks inside the card must not reach the backdrop handler.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        // Focus trap: Tab / Shift-Tab cycle through the dialog's own controls only.
                        .on_action({
                            let card = focus.clone();
                            move |_: &FocusNext, w, cx| {
                                trap_tab(&card, true, w, cx);
                                cx.stop_propagation();
                            }
                        })
                        .on_action({
                            let card = focus.clone();
                            move |_: &FocusPrev, w, cx| {
                                trap_tab(&card, false, w, cx);
                                cx.stop_propagation();
                            }
                        })
                        .on_key_down(move |ev, w, cx| {
                            if ev.keystroke.key == "escape" {
                                if let Some(f) = &key_close {
                                    f(w, cx);
                                    cx.stop_propagation();
                                }
                            }
                        })
                        .when(self.title.is_some() || show_x, |d| {
                            d.child(
                                div()
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(16.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .min_w_0()
                                            .when_some(self.title, |d, s| {
                                                d.child(
                                                    div()
                                                        .text_size(px(20.))
                                                        .font_weight(FontWeight::BOLD)
                                                        .child(s),
                                                )
                                            })
                                            .when_some(self.description, |d, s| {
                                                d.child(div().text_color(t.text_muted).child(s))
                                            }),
                                    )
                                    .when(show_x && x_close.is_some(), |d| {
                                        let f = x_close.clone().unwrap();
                                        d.child(
                                            div()
                                                .id("dialog-x")
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .size(px(34.))
                                                .rounded(px(9.))
                                                .border_1()
                                                .border_color(t.border)
                                                .bg(t.panel)
                                                .cursor_pointer()
                                                .hover(move |s| s.bg(t.hover_strong))
                                                .child(
                                                    icon(Icon::X).size(px(18.)).text_color(t.text),
                                                )
                                                .on_click(move |_, w, cx| f(w, cx)),
                                        )
                                    }),
                            )
                        })
                        .children(self.children)
                        .when(!self.footer.is_empty(), |d| {
                            d.child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(px(10.))
                                    .children(self.footer),
                            )
                        }),
                ),
        )
        .with_priority(10)
    }
}

struct DialogState {
    focus: FocusHandle,
    placed: bool,
}

/// Move focus to the next/previous tab stop *inside* the dialog (wrapping). The global tab order
/// also contains the controls behind the backdrop; those are skipped by stepping until the focus
/// lands in the card again. If the dialog has no tab stop the focus stays where it was.
pub(crate) fn trap_tab(card: &FocusHandle, forward: bool, window: &mut Window, cx: &mut App) {
    let start = window.focused(cx);
    // A bounded walk: the global tab order is small, and a dialog without stops must not loop.
    for _ in 0..256 {
        if forward {
            window.focus_next();
        } else {
            window.focus_prev();
        }
        if !card.is_focused(window) && card.contains_focused(window, cx) {
            return;
        }
    }
    match start {
        Some(h) => window.focus(&h),
        None => window.focus(card),
    }
}

/// Focus the dialog's card once (and again if focus left the dialog entirely).
fn grab_focus(focus: &FocusHandle, window: &mut Window, cx: &mut App) {
    if !focus.contains_focused(window, cx) {
        window.focus(focus);
    }
}

/// Yes/No confirmation (legacy: "Reset to Stock", "Uninstall ..." prompts).
///
/// ```ignore
/// confirm_dialog("confirm-reset", "Reset to stock?", "This removes all mods.", "Reset", true,
///     move |_w, cx| { /* confirmed */ }, move |_w, cx| { /* cancelled */ })
/// ```
pub fn confirm_dialog(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    confirm_label: impl Into<SharedString>,
    destructive: bool,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
) -> Dialog {
    let cancel = std::rc::Rc::new(on_cancel);
    let c2 = cancel.clone();
    Dialog::new(id)
        .title(title)
        .description(message)
        .width(px(420.))
        .no_close_button()
        .on_close(move |w, cx| cancel(w, cx))
        // Destructive prompts start on Cancel, the others on the confirm button.
        .initial_focus(usize::from(!destructive))
        .footer(Button::new("confirm-cancel", "Cancel").on_click(move |_, w, cx| c2(w, cx)))
        .footer(
            Button::new("confirm-ok", confirm_label)
                .kind(if destructive {
                    ButtonKind::Danger
                } else {
                    ButtonKind::Primary
                })
                .on_click(move |_, w, cx| on_confirm(w, cx)),
        )
}
