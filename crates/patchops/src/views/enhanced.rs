//! Enhanced tab (legacy `activeView === "enhanced"`, main.tsx ~L1483-1700). TO BE FILLED IN.
//!
//! What goes here: BO3 Enhanced status card (`Metric`s), install-source card (`DropZone` for
//! DUMP.zip / folder, path `TextInput` seeded from `state.enhanced.dump_source`, Browse via
//! `controller.open_browser(BrowseTarget::DumpFolder, Some(current), cx)` and a subscription to
//! `ControllerEvent::DirSelected`, Validate = `Action::EnhancedValidate` whose result arrives as
//! `ControllerEvent::Finished` with `ReplyData::Validated`), Install (`Action::EnhancedInstall`),
//! Uninstall (`views::run_confirmed(Action::EnhancedUninstall)`).

use gpui::{div, prelude::*, px, Context, Entity, IntoElement, Render, Subscription, Window};

use crate::controller::Controller;
use crate::theme::ActiveTheme;
use crate::views;
use crate::widgets::Panel;

pub struct EnhancedView {
    c: Entity<Controller>,
    _sub: Subscription,
}

impl EnhancedView {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        let _sub = views::watch(&controller, cx);
        Self { c: controller, _sub }
    }
}

impl Render for EnhancedView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let _state = self.c.read(cx).state().cloned();
        div().size_full().flex().flex_col().child(
            Panel::new("Enhanced").child(
                div().text_color(t.text_muted).text_size(px(t.font_sm)).child("This tab is not implemented yet."),
            ),
        )
    }
}
