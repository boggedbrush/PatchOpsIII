//! Confirmation and Steam-depot dialogs (`.modal-backdrop`, `.depot-modal`).
use super::components::{Btn, Glyph, icon, modal, modal_surface};
use super::{ControlCenter, theme};
use gpui::{prelude::*, *};
use std::time::Instant;

fn code_box(text: impl Into<SharedString>) -> Div {
    div()
        .p(px(12.))
        .border_1()
        .border_color(theme::border())
        .rounded(theme::radius_card())
        .bg(theme::scrim().alpha(0.35))
        .text_size(px(theme::FONT_SM))
        .child(text.into())
}

impl ControlCenter {
    /// Every staged action asks for confirmation before it reaches the service.
    pub(super) fn confirm_modal(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (label, requests) = self.pending.as_ref()?;
        let steps = requests.len();
        let target = self.value("/gameDir");
        Some(modal(
            modal_surface()
                .child(
                    div()
                        .text_size(px(theme::FONT_LG + 2.))
                        .font_weight(FontWeight::BOLD)
                        .child("Confirm operation"),
                )
                .child(div().text_color(theme::muted_strong()).child(if steps > 1 {
                    format!("{label} ({steps} steps)")
                } else {
                    label.clone()
                }))
                .child(code_box(format!("Target: {target}")))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(10.))
                        .child(Btn::new("cancel", "Cancel").on_click(cx.listener(
                            |view, _: &ClickEvent, _, cx| {
                                view.pending = None;
                                cx.notify();
                            },
                        )))
                        .child(
                            Btn::new("confirm", "Confirm")
                                .primary()
                                .disabled(self.busy)
                                .on_click(
                                    cx.listener(|view, _: &ClickEvent, _, cx| view.confirm(cx)),
                                ),
                        ),
                ),
        ))
    }

    /// Shown when the compatible EXE needs the Steam depot first.
    pub(super) fn depot_modal(&self, cx: &mut Context<Self>) -> Option<Div> {
        let depot = self.depot.as_ref()?;
        let watching = depot.watching;
        let copied = depot.copied;
        Some(modal(
            modal_surface()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(54.))
                        .border_1()
                        .border_color(theme::border())
                        .rounded(theme::radius_card())
                        .child(icon(Glyph::Clipboard, 28., theme::accent())),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div()
                                .text_size(px(theme::FONT_LG + 2.))
                                .font_weight(FontWeight::BOLD)
                                .child("Copy this"),
                        )
                        .child(
                            div().child(
                                "Run this command in the Steam console to download the compatible build.",
                            ),
                        )
                        .child(code_box(depot.command.clone()))
                        .when(watching, |this| {
                            this.child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::ok())
                                    .child(
                                        "Watching for the depot. PatchOpsIII will install it when ready.",
                                    ),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(
                            Btn::new("depot-cancel", "Cancel").grow().on_click(cx.listener(
                                |view, _: &ClickEvent, _, cx| {
                                    view.depot = None;
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            Btn::new("depot-copy", if copied { "Copied" } else { "Copy" })
                                .grow()
                                .icon(Glyph::Clipboard)
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    if let Some(depot) = view.depot.as_mut() {
                                        depot.copied = true;
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            depot.command.clone(),
                                        ));
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            Btn::new("depot-continue", "Continue")
                                .grow()
                                .primary()
                                .icon(Glyph::ExternalLink)
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    cx.open_url("steam://open/console");
                                    if let Some(depot) = view.depot.as_mut() {
                                        depot.watching = true;
                                        depot.last_poll = Instant::now();
                                    }
                                    cx.notify();
                                })),
                        ),
                ),
        ))
    }
}
