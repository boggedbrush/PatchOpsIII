//! T7 Patch: install state, gamertag and network security.
use super::components::{
    Btn, Glyph, boxed_metric, chip, column, columns, field_label, icon_button, inline_pill,
    module_row, panel, switch, text_xs,
};
use super::{ControlCenter, theme};
use gpui::{prelude::*, *};
use serde_json::json;

const GAMERTAG_COLORS: [(&str, &str, u32); 10] = [
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

fn color_for(code: &str) -> (&'static str, Hsla) {
    let (_, label, hex) = GAMERTAG_COLORS
        .iter()
        .find(|(candidate, ..)| *candidate == code)
        .unwrap_or(&GAMERTAG_COLORS[0]);
    (label, rgb(*hex).into())
}

/// `.compact-setting`: a bordered row with a label and a switch.
fn compact_setting(label: &str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .min_h(px(44.))
        .px(px(14.))
        .border_1()
        .border_color(theme::border())
        .rounded(theme::radius_card())
        .bg(theme::white(0.032))
        .child(div().flex_1().min_w_0().child(label.to_owned()))
        .child(control)
}

fn tone(ok: bool) -> Hsla {
    if ok {
        theme::ok()
    } else {
        theme::muted_strong()
    }
}

impl ControlCenter {
    fn t7_overview(&self, cx: &mut Context<Self>) -> Div {
        let installed = self.flag("/t7/installed");
        let conf = self.flag("/t7/confExists");
        let mode = self.value("/t7/mode");
        let divider = |content: Div, min: f32| {
            content
                .flex_1()
                .min_w(px(min))
                .border_r_1()
                .border_color(theme::border())
        };
        panel(
            "T7 Patch",
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .child(divider(
                    module_row(Glyph::Shield, "T7 Patch", installed),
                    240.,
                ))
                .child(divider(
                    inline_pill(
                        "Config",
                        if conf {
                            "t7patch.conf found"
                        } else {
                            "Config missing"
                        },
                        conf,
                    ),
                    230.,
                ))
                .child(divider(
                    inline_pill("Game Mode", mode.clone(), mode != "Unknown" && mode != "—"),
                    170.,
                ))
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w(px(440.))
                        .gap(px(10.))
                        .p(px(10.))
                        .child(
                            self.action(
                                cx,
                                "t7-install",
                                "Install / Update T7 Patch",
                                "/api/t7-install",
                                json!({}),
                            )
                            .icon(Glyph::Download)
                            .primary()
                            .grow(),
                        )
                        .child(
                            self.action(
                                cx,
                                "t7-uninstall",
                                "Uninstall T7 Patch",
                                "/api/t7-uninstall",
                                json!({}),
                            )
                            .icon(Glyph::Trash)
                            .disabled(!self.can_act() || !installed)
                            .grow(),
                        ),
                ),
        )
    }

    fn t7_gamertag_panel(&self, cx: &mut Context<Self>) -> Div {
        let conf = self.flag("/t7/confExists");
        let saved_name = self.string("/t7/plainName");
        let saved_color = self.string("/t7/colorCode");
        let draft = self.text("gamertag", cx);
        let preview = if draft.trim().is_empty() {
            if saved_name.is_empty() {
                "None".to_owned()
            } else {
                saved_name.clone()
            }
        } else {
            draft.trim().to_owned()
        };
        let pending = draft != saved_name || self.t7_color != saved_color;
        let (_, preview_color) = color_for(&self.t7_color);
        let (_, saved_swatch) = color_for(&saved_color);
        let saved_display = if saved_name.is_empty() {
            "None".to_owned()
        } else {
            saved_name.clone()
        };
        let enabled = conf && !self.busy;
        panel(
            "Gamertag",
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .p(px(14.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .min_h(px(82.))
                        .p(px(14.))
                        .border_1()
                        .border_color(theme::white(0.12))
                        .rounded(theme::radius_card())
                        .bg(theme::white(0.035))
                        .child(
                            div()
                                .max_w_full()
                                .truncate()
                                .text_size(px(30.))
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .text_color(preview_color)
                                .child(preview),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(field_label("Name"))
                        .child(self.field("gamertag").disabled(!enabled)),
                )
                .child(div().flex().flex_col().gap(px(7.)).children(
                    GAMERTAG_COLORS.chunks(5).enumerate().map(|(row, colors)| {
                        div()
                            .flex()
                            .gap(px(7.))
                            .children(colors.iter().enumerate().map(|(col, (code, label, hex))| {
                                let code = (*code).to_owned();
                                chip(
                                    ("color", row * 5 + col),
                                    *label,
                                    Some(rgb(*hex).into()),
                                    self.t7_color == code,
                                    enabled,
                                    cx.listener(move |view, _: &ClickEvent, _, cx| {
                                        view.t7_color = code.clone();
                                        cx.notify();
                                    }),
                                )
                                .flex_1()
                                .text_size(px(11.))
                            }))
                    }),
                ))
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .flex_1()
                                .min_w_0()
                                .p(px(10.))
                                .border_1()
                                .border_color(theme::border())
                                .rounded(theme::radius_card())
                                .bg(theme::white(0.026))
                                .child(text_xs("Currently Saved", theme::muted()).flex_none())
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .flex_1()
                                        .min_w_0()
                                        .gap(px(10.))
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(px(theme::FONT_XS))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(theme::muted_strong())
                                                .child(saved_display),
                                        )
                                        .child(
                                            div()
                                                .size(px(12.))
                                                .flex_none()
                                                .rounded_full()
                                                .border_1()
                                                .border_color(theme::white(0.42))
                                                .bg(saved_swatch),
                                        )
                                        .child(
                                            div()
                                                .ml_auto()
                                                .flex_none()
                                                .text_size(px(theme::FONT_XS))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(theme::muted_strong())
                                                .child(format!(
                                                    "{}/20",
                                                    saved_name.chars().count()
                                                )),
                                        ),
                                ),
                        )
                        .child(
                            boxed_metric(
                                "Status",
                                if pending { "Unsaved" } else { "Saved" },
                                if pending {
                                    theme::warning()
                                } else {
                                    theme::ok()
                                },
                            )
                            .flex_none()
                            .w(px(120.)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(
                            Btn::new("t7-reset-name", "Reset Edits")
                                .compact()
                                .grow()
                                .disabled(!pending || self.busy)
                                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                                    view.load_t7(window, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Btn::new("t7-save-name", "Update Gamertag")
                                .compact()
                                .icon(Glyph::Save)
                                .grow()
                                .disabled(!self.can_act() || !conf)
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    let name = view.text("gamertag", cx);
                                    let name = name.trim().to_owned();
                                    if name.is_empty() {
                                        view.reject("Gamertag cannot be empty.", cx);
                                    } else if name.chars().count() > 20 {
                                        view.reject("Gamertag cannot exceed 20 characters.", cx);
                                    } else {
                                        let color = view.t7_color.clone();
                                        view.stage(
                                            "Update T7 gamertag".into(),
                                            "/api/t7-config".into(),
                                            json!({"gamertag": name, "colorCode": color}),
                                            cx,
                                        );
                                    }
                                })),
                        ),
                ),
        )
    }

    fn t7_security_panel(&self, cx: &mut Context<Self>) -> Div {
        let conf = self.flag("/t7/confExists");
        let friends = self.flag("/t7/friendsOnly");
        let saved_password = self.string("/t7/networkPassword");
        let has_password = !saved_password.is_empty();
        let enabled = self.t7_password_enabled;
        let controls = conf && enabled;
        let pending = enabled != has_password || !self.text("password", cx).is_empty();
        let show_current = self.show_current_password;
        let show_new = self.show_new_password;
        let current_display = if saved_password.is_empty() {
            ("None".to_owned(), theme::muted())
        } else if show_current {
            (saved_password.clone(), theme::text())
        } else {
            (
                "•".repeat(saved_password.chars().count().min(24)),
                theme::text(),
            )
        };
        panel(
            "Security",
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .p(px(14.))
                .child(compact_setting(
                    "Friends Only Mode",
                    switch(
                        "t7-friends",
                        friends,
                        conf && self.can_act(),
                        self.stage_click(
                            cx,
                            format!(
                                "{} Friends Only mode",
                                if friends { "Disable" } else { "Enable" }
                            ),
                            "/api/t7-config",
                            json!({"friendsOnly": !friends}),
                        ),
                    ),
                ))
                .child(compact_setting(
                    "Network Password",
                    switch(
                        "t7-password-toggle",
                        enabled,
                        conf && !self.busy && self.pending.is_none(),
                        cx.listener(|view, _: &ClickEvent, _, cx| {
                            if view.t7_password_enabled {
                                view.stage(
                                    "Remove the T7 network password".into(),
                                    "/api/t7-config".into(),
                                    json!({"networkPassword": ""}),
                                    cx,
                                );
                            } else {
                                view.t7_password_enabled = true;
                                view.t7_password_touched = true;
                                cx.notify();
                            }
                        }),
                    ),
                ))
                .child(
                    columns()
                        .child(
                            column(220.)
                                .gap(px(6.))
                                .child(field_label("Current Password"))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .flex_1()
                                                .min_w_0()
                                                .min_h(px(theme::CONTROL_HEIGHT - 2.))
                                                .px(px(10.))
                                                .border_1()
                                                .border_color(theme::border())
                                                .rounded(theme::radius_card())
                                                .bg(theme::field())
                                                .text_color(current_display.1)
                                                .when(!controls, |this| this.opacity(0.58))
                                                .child(div().truncate().child(current_display.0)),
                                        )
                                        .child(icon_button(
                                            "show-current-password",
                                            if show_current { Glyph::EyeOff } else { Glyph::Key },
                                            controls,
                                            cx.listener(|view, _: &ClickEvent, _, cx| {
                                                view.show_current_password = !view.show_current_password;
                                                cx.notify();
                                            }),
                                        )),
                                ),
                        )
                        .child(
                            column(220.)
                                .gap(px(6.))
                                .child(field_label("Password"))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .child(self.field("password").disabled(self.busy || !controls)),
                                        )
                                        .child(icon_button(
                                            "show-new-password",
                                            if show_new { Glyph::EyeOff } else { Glyph::Key },
                                            controls,
                                            cx.listener(|view, _: &ClickEvent, window, cx| {
                                                view.show_new_password = !view.show_new_password;
                                                let masked = !view.show_new_password;
                                                view.inputs["password"].update(cx, |state, cx| {
                                                    state.set_masked(masked, window, cx)
                                                });
                                                cx.notify();
                                            }),
                                        )),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(boxed_metric(
                            "Friends Only",
                            if friends { "Enabled" } else { "Disabled" },
                            tone(friends),
                        ))
                        .child(boxed_metric(
                            "Password",
                            if has_password { "Set" } else { "Not set" },
                            tone(has_password),
                        ))
                        .child(boxed_metric(
                            "Status",
                            if pending { "Unsaved" } else { "Saved" },
                            if pending { theme::warning() } else { theme::ok() },
                        )),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(
                            Btn::new("t7-reset-security", "Reset Edits")
                                .compact()
                                .grow()
                                .disabled(!pending || self.busy)
                                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                                    view.t7_password_touched = false;
                                    view.t7_password_enabled = !view.string("/t7/networkPassword").is_empty();
                                    view.set_text("password", String::new(), window, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Btn::new("t7-save-password", "Update Security")
                                .compact()
                                .icon(Glyph::Save)
                                .grow()
                                .disabled(!self.can_act() || !controls)
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    let password = view.text("password", cx);
                                    let password = password.trim().to_owned();
                                    if password.is_empty() {
                                        view.reject(
                                            "Enter a network password, or turn the toggle off to remove it.",
                                            cx,
                                        );
                                    } else {
                                        view.stage(
                                            "Update T7 network password".into(),
                                            "/api/t7-config".into(),
                                            json!({"networkPassword": password}),
                                            cx,
                                        );
                                    }
                                })),
                        ),
                ),
        )
    }

    pub(super) fn t7_view(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(theme::PANEL_GAP))
            .child(self.t7_overview(cx))
            .child(
                columns()
                    .items_start()
                    .child(column(340.).child(self.t7_gamertag_panel(cx)))
                    .child(column(340.).child(self.t7_security_panel(cx))),
            )
            .into_any_element()
    }
}
