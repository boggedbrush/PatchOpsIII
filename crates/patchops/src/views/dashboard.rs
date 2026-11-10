//! Dashboard tab (legacy `activeView === "dashboard"`): Status Overview, Quality of Life
//! checkboxes (incl. "Apply All") and Launch Options. This file is the REFERENCE for the view
//! conventions described in `GPUI_NOTES.md` ("Writing a view"):
//!
//! * the view struct holds `Entity<Controller>` + only *local UI state* (here: the selected
//!   radio); everything else is read from the controller's `Arc<State>` in `render`;
//! * `views::watch` re-renders the view on every controller change;
//! * buttons / checkboxes are one-liners through `views::run` / `views::run_bool`;
//! * `.busy(c.is_busy("key"))` shows the legacy busy state.

use gpui::{div, prelude::*, px, Context, Entity, FontWeight, IntoElement, Render, SharedString, Subscription, Window};

use crate::actions::Action;
use crate::controller::Controller;
use crate::theme::ActiveTheme;
use crate::views::{self, PANEL_GAP};
use crate::widgets::{Button, Checkbox, Panel, Radio, ScrollArea, StatusRow, Tone};

pub struct DashboardView {
    c: Entity<Controller>,
    /// Selected launch profile id (radio). Follows `state.active_launch_profile` whenever that
    /// changes (legacy effect on `[state.activeLaunchProfile]`; "custom" selects "default").
    selected: String,
    synced_active: Option<String>,
    _sub: Subscription,
}

impl DashboardView {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        // State -> local selection sync happens when the controller notifies, not in render.
        let _sub = cx.observe(&controller, |this, c, cx| {
            let active = c.read(cx).state().map(|s| s.active_launch_profile.clone());
            if active != this.synced_active {
                if let Some(a) = &active {
                    this.selected = if a == "custom" { "default".into() } else { a.clone() };
                }
                this.synced_active = active;
            }
            cx.notify();
        });
        let mut this = Self { c: controller, selected: "default".into(), synced_active: None, _sub };
        let active = this.c.read(cx).state().map(|s| s.active_launch_profile.clone());
        if let Some(a) = &active {
            this.selected = if a == "custom" { "default".into() } else { a.clone() };
        }
        this.synced_active = active;
        this
    }
}

/// Legacy `StatusRow`: value is the detail (default "Configured") when ok, else "Not configured".
fn status_row(label: &str, ok: bool, detail: Option<String>) -> StatusRow {
    StatusRow::new(label.to_string(), Tone::from_ok(ok))
        .value(if ok { detail.unwrap_or_else(|| "Configured".into()) } else { "Not configured".into() })
}

impl Render for DashboardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let cw = views::content_width(window);
        let ctl = self.c.read(cx);
        let Some(state) = ctl.state().cloned() else {
            return div().into_any_element();
        };
        let busy = |k: &'static str| ctl.is_busy(k);
        let c = self.c.clone();

        // ---- Status Overview --------------------------------------------------------------
        let active_label = match state.active_launch_profile.as_str() {
            "custom" => Some("Custom".to_string()),
            "default" => None,
            id => state.launch_profiles.iter().find(|p| p.id == id).map(|p| p.label.clone()),
        };
        let qol = &state.qol;
        let status = Panel::new("Status Overview")
            .child(status_row("T7 Patch", state.mods.t7_patch, None))
            .child(status_row("DXVK-GPLAsync", state.mods.dxvk, None))
            .child(status_row("BO3 Enhanced", state.mods.enhanced, None))
            .child(status_row("Launch Option", state.active_launch_profile != "default", active_label))
            .child(status_row("Quality of Life", qol.d3dcompiler || qol.intro || qol.all_intros, None).last());

        // ---- Quality of Life --------------------------------------------------------------
        let all_on = qol.d3dcompiler && qol.intro && qol.all_intros;
        let check_row = |id: &'static str, on: bool, label: &'static str, make: fn(bool) -> Action| {
            div().min_h(px(34.)).flex().items_center().child(
                Checkbox::new(id, on).label(label).on_change(views::run_bool(&c, make)),
            )
        };
        let qol_panel = Panel::new("Quality of Life")
            .child(check_row("qol-all", all_on, "Apply All", Action::QolAll))
            .child(check_row(
                "qol-d3d",
                qol.d3dcompiler,
                "Use latest d3dcompiler (d3dcompiler_46.dll)",
                Action::D3dCompiler,
            ))
            .child(check_row(
                "qol-intro",
                qol.intro,
                "Skip Intro (BO3_Global_Logo_LogoSequence.mkv)",
                Action::IntroSkip,
            ))
            .child(check_row(
                "qol-all-intros",
                qol.all_intros,
                "Skip All Intros (Campaign, Zombies, etc.)",
                Action::AllIntrosSkip,
            ));

        // ---- Launch Options ---------------------------------------------------------------
        let selected = state.launch_profiles.iter().find(|p| p.id == self.selected);
        let installable = selected.is_some_and(|p| p.id != "default" && p.id != "offline");
        let n = state.launch_profiles.len();
        let rows = state.launch_profiles.iter().enumerate().map(|(i, p)| {
            let this = cx.entity();
            let id = p.id.clone();
            let tag = (p.id != "default" && p.id != "offline").then(|| {
                let tone = if p.installed {
                    Tone::Ok
                } else if p.subscribed {
                    Tone::Warn
                } else {
                    Tone::Danger
                };
                // Legacy `.profile-state`: right-aligned at its natural width; below 840px it becomes a
                // 92px word-wrapping column (never ellipsised or cut mid-word).
                div()
                    .flex_none()
                    .when(cw < 608., |d| d.w(px(120.)).line_height(px(15.)))
                    .text_right()
                    .text_size(px(t.font_sm))
                    .font_weight(FontWeight::BOLD)
                    .text_color(tone.color(&t))
                    .child(SharedString::from(p.state.clone()))
            });
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(32.))
                .when(i + 1 < n, |d| d.border_b_1().border_color(t.border))
                .child(
                    div().flex_1().min_w_0().child(
                        Radio::new(("profile", i), self.selected == p.id).label(p.label.clone()).on_select(
                            move |_, cx| {
                                this.update(cx, |v, cx| {
                                    v.selected = id.clone();
                                    cx.notify();
                                })
                            },
                        ),
                    ),
                )
                .children(tag)
        });
        let btn = |b: Button| div().flex_1().min_w(px(110.)).child(b.small().full_width());
        // Handlers read the selection (and its launch option) at click time, not render time.
        let launch_panel = Panel::new("Launch Options")
            .child(div().flex().flex_col().children(rows))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(10.))
                    .pt(px(12.))
                    .child(btn(
                        Button::new("profile-install", "Install Selected Mod")
                            .disabled(!installable)
                            .busy(busy("workshop-install"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let id = this.selected.clone();
                                this.c.update(cx, |c, cx| c.run(Action::WorkshopInstall(id), cx));
                            })),
                    ))
                    .child(btn(
                        Button::new("profile-refresh", "Refresh")
                            .busy(busy("refresh"))
                            .on_click(views::run(&c, Action::Refresh)),
                    ))
                    .child(btn(
                        Button::new("profile-apply", "Apply")
                            .busy(busy("launch-options"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let option = this
                                    .c
                                    .read(cx)
                                    .state()
                                    .and_then(|s| s.launch_profiles.iter().find(|p| p.id == this.selected))
                                    .map(|p| p.option.clone())
                                    .unwrap_or_default();
                                this.c.update(cx, |c, cx| {
                                    c.run(Action::LaunchOptions { options: option, preserve_fs_game: false }, cx)
                                });
                            })),
                    )),
            );

        ScrollArea::new("dashboard-scroll").gap(px(PANEL_GAP))
            .child(div().flex_none().min_h(px(190.)).child(status))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .flex_wrap()
                    // Legacy `.dashboard-split { align-items: end }` (app.css:899): the shorter panel
                    // sits on the row's bottom edge; `start` once stacked (<= 980px rules).
                    .when(cw >= 760., |d| d.items_end())
                    .gap(px(PANEL_GAP))
                    .child(div().flex_1().min_w(px(360.)).child(qol_panel))
                    .child(div().flex_1().min_w(px(360.)).child(launch_panel)),
            )
            .into_any_element()
    }
}
