//! T7 Patch tab (legacy `activeView === "t7"`, main.tsx ~L1235-1393).
//!
//! * Overview panel: module row, "Config" / "Game Mode" pills, Install / Update (with download
//!   progress) and Uninstall (confirmed).
//! * Gamertag panel: live preview, name input (max 20), colour chips, "Currently Saved", Saved /
//!   Unsaved indicator, Reset Edits / Update Gamertag.
//! * Security panel: Friends Only, Network Password toggle (whole row clickable), masked
//!   current/new password inputs with reveal buttons, state summary, Reset Edits / Update Security.
//!
//! Local form state (name, colour, password, password-enabled, "touched") lives here and is
//! re-seeded from `State.t7` exactly where the legacy `useEffect([state.t7, t7PasswordTouched])`
//! does: whenever the controller replaces its state (every action result / refresh), and when
//! the "touched" flag flips back to false. The password pair is skipped while touched.

use std::sync::Arc;

use gpui::{
    div, prelude::*, px, rgb, Context, CursorStyle, Entity, FontWeight, Hsla, IntoElement, Render, SharedString,
    Subscription, Window,
};
use patchops_core::State;

use crate::actions::Action;
use crate::controller::{Controller, ControllerEvent};
use crate::theme::ActiveTheme;
use crate::views::{self, PANEL_GAP};
use crate::widgets::{
    icon, Button, Chip, Icon, IconButton, Panel, ProgressBar, ScrollArea, StatusPill, TextInput, TextInputEvent, Toggle,
    Tone,
};

/// Legacy `gamertagColors` (code, label, swatch).
const COLORS: [(&str, &str, u32); 10] = [
    ("", "Default", 0xf6f6f6),
    ("^1", "Red", 0xff453a),
    ("^2", "Green", 0x34c759),
    ("^3", "Yellow", 0xffd60a),
    ("^4", "Blue", 0x5ac8fa),
    ("^5", "Cyan", 0x64d2ff),
    ("^6", "Pink", 0xff2d55),
    ("^8", "Mid Blue", 0x0a84ff),
    ("^9", "Cinnabar", 0xff6b35),
    ("^0", "Black", 0x151518),
];

fn color_of(code: &str) -> (&'static str, Hsla) {
    let (_, label, hex) = COLORS.iter().find(|c| c.0 == code).copied().unwrap_or(COLORS[0]);
    (label, Hsla::from(rgb(hex)))
}

pub struct T7View {
    c: Entity<Controller>,
    name: Entity<TextInput>,
    password: Entity<TextInput>,
    current_password: Entity<TextInput>,
    color: String,
    pw_enabled: bool,
    pw_touched: bool,
    show_current: bool,
    show_edit: bool,
    /// The state the form was last seeded from (pointer identity = legacy `state.t7` identity).
    seeded: Option<Arc<State>>,
    _subs: Vec<Subscription>,
}

impl T7View {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| {
            let mut i = TextInput::new("Gamertag", cx);
            i.set_max_len(Some(20));
            i
        });
        let password = cx.new(|cx| {
            let mut i = TextInput::new("Enter network password", cx);
            i.set_masked(true, cx);
            i
        });
        let current_password = cx.new(|cx| {
            let mut i = TextInput::new("None", cx);
            i.set_masked(true, cx);
            i.set_read_only(true, cx);
            i
        });
        let mut subs = Vec::new();
        subs.push(cx.observe(&controller, |this, _c, cx| {
            this.sync(false, cx);
            cx.notify();
        }));
        subs.push(cx.subscribe(&controller, |this, _c, ev: &ControllerEvent, cx| {
            // Legacy `updateT7Password`: `setT7PasswordTouched(false)` after the request (any outcome).
            if let ControllerEvent::Finished(f) = ev {
                if f.key == "t7-password" {
                    this.pw_touched = false;
                    this.sync(true, cx);
                    cx.notify();
                }
            }
        }));
        subs.push(cx.subscribe(&name, |_, _, _: &TextInputEvent, cx| cx.notify()));
        subs.push(cx.subscribe(&password, |this, _, ev: &TextInputEvent, cx| {
            if matches!(ev, TextInputEvent::Changed(_)) {
                this.pw_touched = true;
            }
            cx.notify();
        }));
        let mut this = Self {
            c: controller,
            name,
            password,
            current_password,
            color: String::new(),
            pw_enabled: false,
            pw_touched: false,
            show_current: false,
            show_edit: false,
            seeded: None,
            _subs: subs,
        };
        this.sync(false, cx);
        this
    }

    /// Legacy effect: copy gamertag + colour (and, unless touched, the password pair) from state.
    fn sync(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(state) = self.c.read(cx).state().cloned() else { return };
        if !force && self.seeded.as_ref().is_some_and(|s| Arc::ptr_eq(s, &state)) {
            return;
        }
        self.seed_gamertag(&state, cx);
        if !self.pw_touched {
            self.seed_password(&state, cx);
        }
        self.seeded = Some(state);
    }

    fn seed_gamertag(&mut self, state: &State, cx: &mut Context<Self>) {
        self.name.update(cx, |i, cx| i.sync_text(&state.t7.plain_name, cx));
        self.color = state.t7.color_code.clone();
    }

    fn seed_password(&mut self, state: &State, cx: &mut Context<Self>) {
        self.password.update(cx, |i, cx| i.sync_text(&state.t7.network_password, cx));
        self.pw_enabled = !state.t7.network_password.is_empty();
    }

    fn set_password_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.pw_enabled = enabled;
        self.c.update(cx, |c, cx| c.dismiss_error(cx));
        if !enabled {
            self.pw_touched = true;
            self.c.update(cx, |c, cx| c.run(Action::T7PasswordOff, cx));
        }
        cx.notify();
    }
}

/// Bordered tile: muted caption over a value (legacy `.t7-state-grid > div`).
fn state_tile(label: &str, value: impl IntoElement, cx: &Context<T7View>) -> gpui::Div {
    let t = *cx.theme();
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(4.))
        .p(px(8.))
        .border_1()
        .border_color(t.border)
        .rounded(px(t.radius - 5.))
        .bg(gpui::white().opacity(0.026))
        .child(
            div()
                .text_size(px(t.font_xs))
                .font_weight(FontWeight::BOLD)
                .text_color(t.text_muted)
                .child(SharedString::from(label.to_string())),
        )
        .child(value)
}

fn state_value(text: &str, color: Hsla, cx: &Context<T7View>) -> gpui::Div {
    let t = *cx.theme();
    div()
        .truncate()
        .text_size(px(t.font_xs))
        .font_weight(FontWeight::BOLD)
        .text_color(color)
        .child(SharedString::from(text.to_string()))
}

/// Label above a control (legacy `<label>` in `.field-grid`).
fn labelled(label: &str, body: impl IntoElement, cx: &Context<T7View>) -> gpui::Div {
    let t = *cx.theme();
    div().flex_1().min_w(px(200.)).flex().flex_col().gap(px(6.)).child(
        div()
            .text_size(px(t.font_xs))
            .font_weight(FontWeight::BOLD)
            .text_color(t.text_muted)
            .child(SharedString::from(label.to_string())),
    )
    .child(body)
}

impl Render for T7View {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let cw = views::content_width(window);
        let ctl = self.c.read(cx);
        let Some(state) = ctl.state().cloned() else {
            return div().into_any_element();
        };
        const KEYS: [&str; 6] = ["t7-install", "t7-uninstall", "t7-gamertag", "t7-password", "t7-password-toggle", "t7-friends"];
        let busy_set: Vec<&'static str> = KEYS.iter().copied().filter(|k| ctl.is_busy(k)).collect();
        let busy = |k: &'static str| busy_set.contains(&k);
        let progress = ctl.progress("t7-install");
        let c = self.c.clone();
        let t7 = &state.t7;
        let conf = t7.conf_exists;

        // ---- derived values (legacy L1066-1072) --------------------------------------------
        let name_text = self.name.read(cx).text().to_string();
        let pw_text = self.password.read(cx).text().to_string();
        let (_, sel_color) = color_of(&self.color);
        let preview_name = {
            let trimmed = name_text.trim();
            if !trimmed.is_empty() {
                trimmed.to_string()
            } else if !t7.plain_name.is_empty() {
                t7.plain_name.clone()
            } else {
                "None".to_string()
            }
        };
        let controls_disabled = !conf || !self.pw_enabled;
        let (saved_label, saved_color) = color_of(&t7.color_code);
        let saved_name = if t7.plain_name.is_empty() { "None".to_string() } else { t7.plain_name.clone() };
        let gamertag_pending = name_text != t7.plain_name || self.color != t7.color_code;
        let has_pw = !t7.network_password.is_empty();
        let security_pending = self.pw_enabled != has_pw || pw_text != t7.network_password;

        // Input enablement (set_disabled is a no-op when unchanged, safe per frame).
        self.name.update(cx, |i, cx| i.set_disabled(!conf, cx));
        self.password.update(cx, |i, cx| i.set_disabled(controls_disabled, cx));
        self.current_password.update(cx, |i, cx| {
            i.set_disabled(controls_disabled, cx);
            i.sync_text(&t7.network_password, cx);
        });

        // ---- Overview ---------------------------------------------------------------------
        let installing = busy("t7-install");
        let module = views::fr(div(), 0.7)
            .min_w(px(205.))
            .min_h(px(42.))
            .flex()
            .items_center()
            .gap(px(9.))
            .px(px(10.))
            .child(icon(Icon::ShieldCheck).size(px(22.)).text_color(t.text_strong))
            .child(div().flex_1().min_w_0().truncate().text_color(t.text_strong).child("T7 Patch"))
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::BOLD)
                    .text_color(if t7.installed { t.ok } else { t.text_muted })
                    .child(if t7.installed { "Configured" } else { "Not configured" }),
            );
        let pill = |p: StatusPill| views::fr(div(), 1.0).min_w(px(192.)).child(p.inline(false).min_h(42.));
        let install = Button::new("t7-install", "Install / Update T7 Patch")
            .primary()
            .icon(Icon::Download)
            .busy(installing)
            .full_width()
            .on_click(views::run(&c, Action::T7Install));
        let uninstall = Button::new("t7-uninstall", "Uninstall T7 Patch")
            .icon(Icon::Trash2)
            .disabled(!t7.installed)
            .busy(busy("t7-uninstall"))
            .full_width()
            .on_click(views::run_confirmed(&c, Action::T7Uninstall));
        // Wide: the legacy fixed 400px actions column. Narrow (legacy `.t7-actions` -> 1 column):
        // the buttons get their own full-width line and wrap again when even that is too tight.
        let mut actions = views::fr(div(), 1.4)
            .min_w(px(456.))
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(6.))
            .p(px(9.))
            .child(
                div()
                    .flex()
                    .when(cw < 1070., |d| d.flex_wrap())
                    .gap(px(10.))
                    .child(div().flex_1().min_w(px(200.)).child(install))
                    .child(div().flex_1().min_w(px(200.)).child(uninstall)),
            );
        if installing {
            actions = actions.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().child(ProgressBar::new("t7-progress").value(progress.map(|p| p as f32 / 100.0))))
                    .child(
                        div()
                            .text_size(px(t.font_xs))
                            .text_color(t.text_muted)
                            .child(SharedString::from(match progress {
                                Some(p) => format!("{p}%"),
                                None => "Downloading...".to_string(),
                            })),
                    ),
            );
        }
        let overview = Panel::new("T7 Patch").flush().child(
            div()
                .flex()
                .when(cw < 1070., |d| d.flex_wrap())
                .items_center()
                .child(module)
                .child(pill(StatusPill::new(
                    "Config",
                    if conf { "t7patch.conf found" } else { "Config missing" },
                    Tone::from_ok(conf),
                )))
                .child(pill(StatusPill::new("Game Mode", t7.mode.clone(), Tone::from_ok(t7.mode != "Unknown"))))
                .child(actions),
        );

        // ---- Gamertag ---------------------------------------------------------------------
        let preview = div()
            .h(px(82.))
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .px(px(14.))
            .border_1()
            .border_color(gpui::white().opacity(0.12))
            .rounded(px(t.radius - 4.))
            .bg(gpui::white().opacity(0.035))
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_size(px(34.))
                    .font_weight(FontWeight::EXTRA_BOLD)
                    .text_color(sel_color)
                    .child(SharedString::from(preview_name)),
            );
        let chip = |i: usize| {
            let (code, label, hex) = COLORS[i];
            let this = cx.entity();
            div().flex_1().min_w_0().child(
                Chip::new(("t7-color", i), label)
                    .height(38.)
                    .swatch(Hsla::from(rgb(hex)))
                    .active(self.color == code)
                    .disabled(!conf)
                    .on_click(move |_, _, cx| {
                        this.update(cx, |v, cx| {
                            v.color = code.to_string();
                            cx.notify();
                        })
                    }),
            )
        };
        let per_row = if (730.0..1000.0).contains(&cw) { 4 } else { 5 };
        let colors = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(
                // 5 per row, except while the two panels share a row in a narrow window (4 per row).
                (0..10usize)
                    .step_by(per_row)
                    .map(|start| div().flex().gap(px(8.)).children((start..(start + per_row).min(10)).map(chip))),
            );
        let saved_tile = views::fr(div(), 2.0)
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(12.))
            .min_h(px(38.))
            .p(px(8.))
            .border_1()
            .border_color(t.border)
            .rounded(px(t.radius - 5.))
            .bg(gpui::white().opacity(0.026))
            .child(
                div()
                    .flex_none()
                    .text_size(px(t.font_xs))
                    .font_weight(FontWeight::BOLD)
                    .text_color(t.text_muted)
                    .child("Currently Saved"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(t.font_xs))
                            .font_weight(FontWeight::BOLD)
                            .text_color(t.text_strong)
                            .child(SharedString::from(saved_name.clone())),
                    )
                    .child(
                        div()
                            .id("t7-saved-swatch")
                            .size(px(12.))
                            .flex_none()
                            .rounded_full()
                            .bg(saved_color)
                            .border_1()
                            .border_color(gpui::white().opacity(0.34))
                            .tooltip(crate::widgets::tooltip(format!("{saved_label} saved color"))),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(t.font_xs))
                            .text_color(t.text_muted)
                            .child(SharedString::from(format!("{}/20", saved_name.chars().count()))),
                    ),
            );
        let status_tile = |pending: bool| {
            state_tile(
                "Status",
                state_value(if pending { "Unsaved" } else { "Saved" }, if pending { t.warning } else { t.ok }, cx),
                cx,
            )
        };
        // Legacy `.t7-state-grid > .inline-state`: "Status" and its value on one line.
        let inline_status = |pending: bool| {
            views::fr(div(), 1.0)
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(10.))
                .min_h(px(38.))
                .p(px(8.))
                .border_1()
                .border_color(t.border)
                .rounded(px(t.radius - 5.))
                .bg(gpui::white().opacity(0.026))
                .child(
                    div().flex_none().text_size(px(t.font_xs)).font_weight(FontWeight::BOLD).text_color(t.text_muted).child("Status"),
                )
                .child(state_value(if pending { "Unsaved" } else { "Saved" }, if pending { t.warning } else { t.ok }, cx))
        };
        let gamertag_actions = div()
            .flex()
            .gap(px(10.))
            .child(views::fr(div(), 0.7).min_w(px(120.)).child(
                Button::new("t7-gt-reset", "Reset Edits")
                    .small()
                    .full_width()
                    .disabled(!gamertag_pending || busy("t7-gamertag"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(s) = this.c.read(cx).state().cloned() {
                            this.seed_gamertag(&s, cx);
                            cx.notify();
                        }
                    })),
            ))
            .child(views::fr(div(), 1.0).min_w(px(160.)).child(
                // Reads the form at click time (a same-frame click can follow the last keystroke).
                Button::new("t7-gt-save", "Update Gamertag")
                    .small()
                    .icon(Icon::Save)
                    .full_width()
                    .disabled(!conf)
                    .busy(busy("t7-gamertag"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let name = this.name.read(cx).text().to_string();
                        let color_code = this.color.clone();
                        this.c.update(cx, |c, cx| c.run(Action::T7Gamertag { name, color_code }, cx));
                    })),
            ));
        let gamertag = Panel::new("Gamertag").child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(preview)
                .child(labelled("Name", self.name.clone(), cx).min_w(px(0.)))
                .child(colors)
                .child(div().flex().gap(px(8.)).child(saved_tile).child(inline_status(gamertag_pending)))
                .child(gamertag_actions),
        );

        // ---- Security ---------------------------------------------------------------------
        let fo_this = c.clone();
        let compact = || {
            div()
                .min_h(px(44.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .px(px(13.))
                .border_1()
                .border_color(t.border)
                .rounded(px(t.radius - 4.))
                .bg(gpui::white().opacity(0.032))
        };
        let friends_row = compact()
            .child(div().text_color(t.text_strong).child("Friends Only Mode"))
            .child(Toggle::new("t7-friends", t7.friends_only).disabled(!conf).on_change(move |v, _, cx| {
                fo_this.update(cx, |c, cx| c.run(Action::T7FriendsOnly(v), cx))
            }));
        let pw_toggle_disabled = !conf || busy("t7-password-toggle");
        let row_this = cx.entity();
        let pw_enabled = self.pw_enabled;
        let password_row = compact()
            .id("t7-pw-row")
            .map(|d| {
                if pw_toggle_disabled {
                    d
                } else {
                    d.cursor(CursorStyle::PointingHand)
                        .hover(move |s| s.bg(t.hover))
                        .on_click(move |_, _, cx| {
                            row_this.update(cx, |v, cx| v.set_password_enabled(!v.pw_enabled, cx))
                        })
                }
            })
            .child(div().text_color(t.text_strong).child("Network Password"))
            // The switch is the keyboard stop (Space / Enter) and toggles by itself; it stops the click
            // from reaching the row so a click on the switch does not toggle twice.
            .child({
                let sw_this = cx.entity();
                Toggle::new("t7-pw-toggle", pw_enabled).disabled(pw_toggle_disabled).on_change(move |v, _, cx| {
                    cx.stop_propagation();
                    sw_this.update(cx, |view, cx| view.set_password_enabled(v, cx));
                })
            });

        let reveal = |id: &'static str, shown: bool, edit: bool| {
            let this = cx.entity();
            div()
                .id(id)
                .tooltip(crate::widgets::tooltip(if shown { "Hide password" } else { "Show password" }))
                .child(IconButton::new((id, 0usize), Icon::KeyRound).active(shown).disabled(controls_disabled).on_click(
                    move |_, _, cx| {
                        this.update(cx, |v, cx| {
                            let input = if edit {
                                v.show_edit = !v.show_edit;
                                (v.password.clone(), v.show_edit)
                            } else {
                                v.show_current = !v.show_current;
                                (v.current_password.clone(), v.show_current)
                            };
                            input.0.update(cx, |i, cx| i.set_masked(!input.1, cx));
                            cx.notify();
                        })
                    },
                ))
        };
        let pw_fields = div()
            .flex()
            .flex_wrap()
            .gap(px(12.))
            .child(labelled(
                "Current Password",
                div()
                    .flex()
                    .gap(px(8.))
                    .items_center()
                    .child(div().flex_1().min_w_0().child(self.current_password.clone()))
                    .child(reveal("t7-reveal-current", self.show_current, false)),
                cx,
            ))
            .child(labelled(
                "Password",
                div()
                    .flex()
                    .gap(px(8.))
                    .items_center()
                    .child(div().flex_1().min_w_0().child(self.password.clone()))
                    .child(reveal("t7-reveal-edit", self.show_edit, true)),
                cx,
            ));
        let on_off = |on: bool, yes: &str, no: &str| (if on { yes } else { no }.to_string(), if on { t.ok } else { t.text_strong });
        let (fo_text, fo_col) = on_off(t7.friends_only, "Enabled", "Disabled");
        let (pw_state, pw_col) = on_off(has_pw, "Set", "Not set");
        let states = div()
            .flex()
            .gap(px(10.))
            .child(div().flex_1().min_w_0().child(state_tile("Friends Only", state_value(&fo_text, fo_col, cx), cx)))
            .child(div().flex_1().min_w_0().child(state_tile("Password", state_value(&pw_state, pw_col, cx), cx)))
            .child(div().flex_1().min_w_0().child(status_tile(security_pending)));
        let security_actions = div()
            .flex()
            .gap(px(10.))
            .child(views::fr(div(), 0.7).min_w(px(120.)).child(
                Button::new("t7-sec-reset", "Reset Edits")
                    .small()
                    .full_width()
                    .disabled(!security_pending || busy("t7-password") || busy("t7-password-toggle"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(s) = this.c.read(cx).state().cloned() {
                            this.seed_password(&s, cx);
                            this.pw_touched = false;
                            cx.notify();
                        }
                    })),
            ))
            .child(views::fr(div(), 1.0).min_w(px(160.)).child(
                Button::new("t7-sec-save", "Update Security")
                    .small()
                    .icon(Icon::Save)
                    .full_width()
                    .disabled(!conf || !self.pw_enabled)
                    .busy(busy("t7-password"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let pw = this.password.read(cx).text().to_string();
                        this.c.update(cx, |c, cx| c.run(Action::T7Password(pw), cx));
                    })),
            ));
        let security = Panel::new("Security").child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(friends_row)
                .child(password_row)
                .child(pw_fields)
                .child(states)
                .child(security_actions),
        );

        ScrollArea::new("t7-scroll")
            .gap(px(PANEL_GAP))
            .child(div().flex_none().child(overview))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .flex_wrap()
                    .items_start()
                    .gap(px(PANEL_GAP))
                    .child(views::fr(div(), 1.05).min_w(px(360.)).child(gamertag))
                    .child(views::fr(div(), 1.0).min_w(px(360.)).child(security)),
            )
            .into_any_element()
    }
}
