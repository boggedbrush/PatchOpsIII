//! Graphics tab (legacy `activeView === "graphics"`, main.tsx ~L1560-1770). TO BE FILLED IN.
//!
//! What goes here: `SubTabs` "Graphics Settings" / "DXVK". Graphics: display mode, resolution,
//! refresh rate, FPS cap, FOV, render resolution, vsync, draw FPS, presets (`Action::ApplyPreset`),
//! advanced (smooth framerate, unlock options, reduce CPU, frame latency, VRAM limit/target,
//! config read-only) all via `Action::SetConfig(ConfigId::..., ConfigInput::..)`,
//! `VramTarget`, `ConfigReadonly`. DXVK: local `DxvkSettings` form (presets Recommended / None,
//! re-seeded from `state.dxvk.settings`), `Action::DxvkConfig` / `DxvkInstall` /
//! `views::run_confirmed(Action::DxvkUninstall)`.

use gpui::{div, prelude::*, px, Context, Entity, IntoElement, Render, Subscription, Window};

use crate::controller::Controller;
use crate::theme::ActiveTheme;
use crate::views;
use crate::widgets::Panel;

pub struct GraphicsView {
    c: Entity<Controller>,
    _sub: Subscription,
}

impl GraphicsView {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        let _sub = views::watch(&controller, cx);
        Self { c: controller, _sub }
    }
}

impl Render for GraphicsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let _state = self.c.read(cx).state().cloned();
        div().size_full().flex().flex_col().child(
            Panel::new("Graphics").child(
                div().text_color(t.text_muted).text_size(px(t.font_sm)).child("This tab is not implemented yet."),
            ),
        )
    }
}
