//! Tools tab (legacy `activeView === "tools"`, main.tsx ~L1773-1850). TO BE FILLED IN.
//!
//! What goes here: cards System (platform, Steam ID, version), Updates (release channel
//! `Segmented` -> `Action::SetReleaseChannel`, Check for Updates -> `Action::UpdateCheck`),
//! Mod Cache, Cache Actions (`run_confirmed(ClearModFiles)`, `run_confirmed(ResetStock)`),
//! Logs (entries = `controller.log_count(cx)`, last error = `controller.last_error_log()`),
//! Log Actions (`Action::CopyLogs`, `Action::ClearLogs`).

use gpui::{div, prelude::*, px, Context, Entity, IntoElement, Render, Subscription, Window};

use crate::controller::Controller;
use crate::theme::ActiveTheme;
use crate::views;
use crate::widgets::Panel;

pub struct ToolsView {
    c: Entity<Controller>,
    _sub: Subscription,
}

impl ToolsView {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        let _sub = views::watch(&controller, cx);
        Self { c: controller, _sub }
    }
}

impl Render for ToolsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let _state = self.c.read(cx).state().cloned();
        div().size_full().flex().flex_col().child(
            Panel::new("Tools").child(
                div().text_color(t.text_muted).text_size(px(t.font_sm)).child("This tab is not implemented yet."),
            ),
        )
    }
}
