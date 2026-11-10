//! T7 Patch tab (legacy `activeView === "t7"`, main.tsx ~L1235-1393). TO BE FILLED IN.
//!
//! What goes here:
//! * Overview panel: module row ("T7 Patch" installed), `StatusPill`s "Config" (`t7.conf_exists`)
//!   and "Game Mode" (`t7.mode`), buttons "Install / Update T7 Patch" (`Action::T7Install`,
//!   busy key `t7-install`) and "Uninstall T7 Patch" (`views::run_confirmed(Action::T7Uninstall)`,
//!   disabled unless `t7.installed`).
//! * Gamertag panel: live preview (name in the chosen colour), name `TextInput` (max 20, disabled
//!   without `t7.conf_exists`), colour `Chip`s (legacy `gamertagColors`), "Currently Saved" line,
//!   Saved/Unsaved indicator, "Reset Edits" and "Update Gamertag" (`Action::T7Gamertag`).
//!   Local form state with dirty tracking: keep `name`/`color` in the view, re-seed them from
//!   `state.t7.plain_name` / `color_code` when the state changes (observe the controller),
//!   pending = differs from state.
//! * Security panel: Friends Only toggle (`Action::T7FriendsOnly`), Network Password toggle
//!   (off -> `Action::T7PasswordOff`), read-only current password with reveal, password
//!   `TextInput` (masked, reveal button), states, "Reset Edits", "Update Security"
//!   (`Action::T7Password`). Password edits are not overwritten by state refreshes while touched.

use gpui::{div, prelude::*, px, Context, Entity, IntoElement, Render, Subscription, Window};

use crate::controller::Controller;
use crate::theme::ActiveTheme;
use crate::views;
use crate::widgets::Panel;

pub struct T7View {
    c: Entity<Controller>,
    _sub: Subscription,
}

impl T7View {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        let _sub = views::watch(&controller, cx);
        Self { c: controller, _sub }
    }
}

impl Render for T7View {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let _state = self.c.read(cx).state().cloned();
        div().size_full().flex().flex_col().child(
            Panel::new("T7 Patch").child(
                div().text_color(t.text_muted).text_size(px(t.font_sm)).child("This tab is not implemented yet."),
            ),
        )
    }
}
