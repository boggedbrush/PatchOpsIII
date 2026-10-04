//! Dashboard: status overview, quality-of-life options and launch options.
use super::components::{Btn, check_row, column, columns, hairline, panel, radio_row, status_row};
use super::progress::Operation;
use super::{ControlCenter, theme};
use crate::backend::Request;
use gpui::{prelude::*, *};
use serde_json::{Value, json};

impl ControlCenter {
    fn active_launch_label(&self) -> Option<String> {
        match self.string("/activeLaunchProfile").as_str() {
            "default" | "" => None,
            "custom" => Some("Custom".into()),
            id => self.state["launchProfiles"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|profile| profile["id"] == id)
                .and_then(|profile| profile["label"].as_str())
                .map(str::to_owned),
        }
    }

    fn status_overview(&self) -> Div {
        let qol_any =
            self.flag("/qol/d3dcompiler") || self.flag("/qol/intro") || self.flag("/qol/allIntros");
        let launch = self.active_launch_label();
        panel(
            "Status Overview",
            div()
                .flex()
                .flex_col()
                .px(px(16.))
                .py(px(10.))
                .child(status_row(
                    "T7 Patch",
                    self.flag("/mods/t7Patch"),
                    None,
                    false,
                ))
                .child(status_row(
                    "DXVK-GPLAsync",
                    self.flag("/mods/dxvk"),
                    None,
                    false,
                ))
                .child(status_row(
                    "BO3 Enhanced",
                    self.flag("/mods/enhanced"),
                    None,
                    false,
                ))
                .child(status_row(
                    "Launch Option",
                    self.string("/activeLaunchProfile") != "default",
                    launch.as_deref(),
                    false,
                ))
                .child(status_row("Quality of Life", qol_any, None, true)),
        )
    }

    fn qol_panel(&self, cx: &mut Context<Self>) -> Div {
        let enabled = self.can_act();
        let options = [
            (
                "d3dcompiler",
                "/qol/d3dcompiler",
                "/api/d3dcompiler",
                "Use latest d3dcompiler (d3dcompiler_46.dll)",
                "the latest d3dcompiler",
            ),
            (
                "intro",
                "/qol/intro",
                "/api/intro-skip",
                "Skip Intro (BO3_Global_Logo_LogoSequence.mkv)",
                "intro skipping",
            ),
            (
                "all-intros",
                "/qol/allIntros",
                "/api/all-intros-skip",
                "Skip All Intros (Campaign, Zombies, etc.)",
                "skipping all intros",
            ),
        ];
        let all = options.iter().all(|(_, pointer, ..)| self.flag(pointer));
        let apply_all = {
            let next = !all;
            let label = format!(
                "{} all quality-of-life options",
                if next { "Enable" } else { "Disable" }
            );
            let paths: Vec<String> = options
                .iter()
                .map(|(_, _, path, ..)| (*path).to_owned())
                .collect();
            cx.listener(move |view, _: &ClickEvent, _, cx| {
                let requests = paths
                    .iter()
                    .map(|path| Request {
                        path: path.clone(),
                        body: Some(json!({"enabled": next})),
                    })
                    .collect();
                view.stage_all(label.clone(), requests, cx);
            })
        };
        panel(
            "Quality of Life",
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .p(px(14.))
                .child(
                    check_row("qol-all", "Apply All", all, enabled, apply_all)
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::muted_strong()),
                )
                .child(hairline().mb(px(4.)))
                .children(
                    options
                        .into_iter()
                        .map(|(id, pointer, path, label, summary)| {
                            let current = self.flag(pointer);
                            check_row(
                                id,
                                label,
                                current,
                                enabled,
                                self.stage_click(
                                    cx,
                                    format!(
                                        "{} {summary}",
                                        if current { "Disable" } else { "Enable" }
                                    ),
                                    path,
                                    json!({"enabled": !current}),
                                ),
                            )
                        }),
                ),
        )
    }

    fn launch_options_panel(&self, cx: &mut Context<Self>) -> Div {
        let profiles: Vec<Value> = self.state["launchProfiles"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let selected = profiles
            .iter()
            .find(|profile| profile["id"] == self.selected_profile.as_str());
        let installable = selected
            .is_some_and(|profile| profile["id"] != "default" && profile["id"] != "offline");
        let selected_option = selected
            .and_then(|profile| profile["option"].as_str())
            .unwrap_or("")
            .to_owned();
        let selected_label = selected
            .and_then(|profile| profile["label"].as_str())
            .unwrap_or("Default")
            .to_owned();
        let can_act = self.can_act();
        panel(
            "Launch Options",
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .p(px(14.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .children(profiles.iter().enumerate().map(|(index, profile)| {
                            let id = profile["id"].as_str().unwrap_or("profile").to_owned();
                            let mut label =
                                profile["label"].as_str().unwrap_or("Profile").to_owned();
                            if profile["active"] == true {
                                label.push_str(" · Active");
                            }
                            let trailing = (id != "default" && id != "offline").then(|| {
                                let installed = profile["installed"] == true;
                                let subscribed = profile["subscribed"] == true;
                                let tone = if installed {
                                    theme::ok()
                                } else if subscribed {
                                    theme::warning()
                                } else {
                                    theme::danger()
                                };
                                (
                                    SharedString::from(
                                        profile["state"].as_str().unwrap_or("").to_owned(),
                                    ),
                                    tone,
                                )
                            });
                            let choice = id.clone();
                            radio_row(
                                ("profile", index),
                                label,
                                self.selected_profile == id,
                                trailing,
                                cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.selected_profile = choice.clone();
                                    cx.notify();
                                }),
                            )
                        })),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            Btn::new("install-profile", "Install Selected Mod")
                                .compact()
                                .grow()
                                .disabled(!can_act || !installable)
                                .on_click(self.stage_click(
                                    cx,
                                    format!("Install {selected_label}"),
                                    "/api/workshop-install",
                                    json!({"profileId": self.selected_profile}),
                                )),
                        )
                        .child(
                            Btn::new("refresh-profiles", "Refresh")
                                .compact()
                                .grow()
                                .disabled(self.busy)
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    view.send("/api/status", None, cx)
                                })),
                        )
                        .child(
                            Btn::new("apply-profile", "Apply")
                                .compact()
                                .grow()
                                .disabled(!can_act || selected.is_none())
                                .on_click(self.stage_click(
                                    cx,
                                    format!("Apply launch option: {selected_label}"),
                                    "/api/launch-options",
                                    json!({"options": selected_option, "preserve_fs_game": false}),
                                )),
                        ),
                )
                .children(self.progress_strip(&[Operation::WorkshopInstall])),
        )
    }

    pub(super) fn dashboard_view(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(theme::PANEL_GAP))
            .child(self.status_overview())
            .child(
                columns()
                    .items_start()
                    .child(column(300.).child(self.qol_panel(cx)))
                    .child(column(300.).child(self.launch_options_panel(cx))),
            )
            .into_any_element()
    }
}
