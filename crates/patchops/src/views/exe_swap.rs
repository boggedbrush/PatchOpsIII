//! EXE Swapper tab (legacy `activeView === "exe"`, main.tsx ~L1395-1481). TO BE FILLED IN.
//!
//! What goes here: the hero (active build title from `exe_swap.profile`/`trusted_executable`,
//! three `StatusPill`s Latest / Compatible / Enhanced) and three option cards (Compatible Build,
//! Latest Build, BO3 Enhanced) with their badges, pro/con lines and buttons:
//! `Action::ExeCompatible` / `ExeCurrent` / `ExeEnhanced` (busy keys `exe-compatible`,
//! `exe-current`, `exe-enhanced`; disabled rules in main.tsx ~L1083-1085).
//!
//! The depot flow is already handled by the controller: when `ExeCompatible` reports
//! `depotRequired` the shell shows the "Copy this" dialog, copies, opens the Steam console and
//! watches for the depot (5 s), then swaps and navigates here. Nothing to do in this view.

use gpui::{div, prelude::*, px, Context, Entity, IntoElement, Render, Subscription, Window};

use crate::controller::Controller;
use crate::theme::ActiveTheme;
use crate::views;
use crate::widgets::Panel;

pub struct ExeSwapView {
    c: Entity<Controller>,
    _sub: Subscription,
}

impl ExeSwapView {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        let _sub = views::watch(&controller, cx);
        Self { c: controller, _sub }
    }
}

impl Render for ExeSwapView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let _state = self.c.read(cx).state().cloned();
        div().size_full().flex().flex_col().child(
            Panel::new("EXE Swapper").child(
                div().text_color(t.text_muted).text_size(px(t.font_sm)).child("This tab is not implemented yet."),
            ),
        )
    }
}
