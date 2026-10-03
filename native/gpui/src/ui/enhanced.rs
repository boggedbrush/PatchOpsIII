//! BO3 Enhanced: dump validation, install, diagnostics and uninstall.
use super::components::{
    Btn, Glyph, bare_panel, card, card_title, column, columns, field_label, icon, metric,
};
use super::{ControlCenter, Validation, format_timestamp, theme};
use gpui::{prelude::*, *};
use serde_json::json;

const DUMP_GUIDE_URL: &str = "https://youtu.be/rBZZTcSJ9_s?si=41p0r_Enten3h5AQ";

fn ok_tone(ok: bool) -> Hsla {
    if ok {
        theme::ok()
    } else {
        theme::muted_strong()
    }
}

fn requirement(label: &str, glyph: Glyph, color: Hsla) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(theme::FONT_SM))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::muted_strong())
        .child(icon(glyph, 15., color))
        .child(label.to_owned())
}

fn summary_line(topic: &str, detail: &str) -> Div {
    div()
        .flex()
        .gap(px(4.))
        .child(
            div()
                .font_weight(FontWeight::EXTRA_BOLD)
                .text_color(theme::text())
                .child(format!("{topic}:")),
        )
        .child(detail.to_owned())
}

impl ControlCenter {
    fn enhanced_status_card(&self) -> Div {
        let installed = self.flag("/enhanced/installed");
        let launch = self.flag("/enhanced/launchOptionsActive");
        let detected = self.flag("/gameDetected");
        let tracked = !self.string("/enhanced/detectedAt").is_empty();
        card()
            .child(card_title(Some(Glyph::Gem), "BO3 Enhanced Status"))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .child(metric(
                        "Status",
                        if installed {
                            "Installed"
                        } else {
                            "Not installed"
                        },
                        ok_tone(installed),
                    ))
                    .child(metric(
                        "Launch override",
                        if launch { "Active" } else { "Inactive" },
                        ok_tone(launch),
                    ))
                    .child(metric(
                        "Game",
                        if detected { "Detected" } else { "Not detected" },
                        ok_tone(detected),
                    ))
                    .child(metric(
                        "Tracking",
                        if tracked { "Tracked" } else { "Not tracked" },
                        ok_tone(tracked),
                    )),
            )
    }

    fn enhanced_source_card(&self, cx: &mut Context<Self>) -> Div {
        let source = self.text("dump", cx);
        let source = source.trim().to_owned();
        let validated = self.validation.ok == Some(true) && self.validation.source == source;
        let can_act = self.can_act();
        let validation_label = match &self.validation.checked_at {
            Some(at) => format!("{} ({at})", self.validation.label),
            None => self.validation.label.clone(),
        };
        let validation_color = match self.validation.ok {
            Some(true) => theme::ok(),
            Some(false) => theme::caution(),
            None => theme::muted_strong(),
        };
        let drop_zone = div()
            .id("dump-drop-zone")
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .flex_1()
            .min_w(px(220.))
            .min_h(px(120.))
            .p(px(14.))
            .border_1()
            .border_dashed()
            .border_color(theme::white(0.2))
            .rounded(theme::radius_card())
            .bg(theme::white(0.02))
            .text_center()
            .drag_over::<ExternalPaths>(|style, _, _, _| {
                style
                    .bg(theme::accent_alpha(0.08))
                    .border_color(theme::accent_alpha(0.6))
            })
            .on_drop(cx.listener(|view, paths: &ExternalPaths, window, cx| {
                if let Some(path) = paths.paths().first() {
                    view.set_text("dump", path.to_string_lossy().into_owned(), window, cx);
                    view.validation = Validation::not_run();
                    cx.notify();
                }
            }))
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child("Drop DUMP.zip or extracted folder here"),
            )
            .child(
                div()
                    .text_size(px(theme::FONT_XS))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::muted())
                    .child("or browse to the dump source manually"),
            );
        let controls = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .flex_1()
            .min_w(px(240.))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        Btn::new("browse-dump", "Browse")
                            .icon(Glyph::FolderOpen)
                            .grow()
                            .disabled(self.busy)
                            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                                view.choose_directory("dump", window, cx)
                            })),
                    )
                    .child(
                        self.action(
                            cx,
                            "validate-dump",
                            "Validate Source",
                            "/api/enhanced-validate",
                            json!({"dumpSource": source}),
                        )
                        .icon(Glyph::CheckCircle)
                        .grow()
                        .disabled(!can_act || source.is_empty()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(field_label("Source"))
                    .child(self.field("dump")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(field_label("Validation"))
                    .child(
                        div()
                            .text_size(px(theme::FONT_SM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(validation_color)
                            .child(validation_label),
                    ),
            )
            .child(
                self.action(
                    cx,
                    "install-enhanced",
                    "Install / Update BO3 Enhanced",
                    "/api/enhanced-install",
                    json!({"dumpSource": source}),
                )
                .icon(Glyph::Download)
                .disabled(!can_act || source.is_empty() || !validated),
            )
            .when(!validated, |this| {
                this.child(
                    div()
                        .text_size(px(theme::FONT_XS))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::muted())
                        .child("Install requires a valid DUMP.zip or extracted UWP dump folder."),
                )
            });
        card()
            .child(card_title(Some(Glyph::Download), "Install Source"))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .child(drop_zone)
                    .child(controls),
            )
    }

    fn enhanced_help_card(&self, cx: &mut Context<Self>) -> Div {
        let detected = self.flag("/gameDetected");
        let dump_ok = self.validation.ok == Some(true);
        card()
            .child(card_title(Some(Glyph::Alert), "Help / Requirements"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .child(requirement(
                        "Game detected",
                        Glyph::CheckCircle,
                        if detected {
                            theme::ok()
                        } else {
                            theme::caution()
                        },
                    ))
                    .child(requirement(
                        "Dump required",
                        Glyph::Alert,
                        if dump_ok {
                            theme::ok()
                        } else {
                            theme::caution()
                        },
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .text_size(px(theme::FONT_XS))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::muted_strong())
                    .child(summary_line("Sources", "DUMP.zip or extracted folder"))
                    .child(summary_line("Checks", "files, read/write, version")),
            )
            .child(
                div().flex().child(
                    Btn::new("dump-guide", "Open Dump Guide")
                        .icon(Glyph::ExternalLink)
                        .on_click(
                            cx.listener(|_, _: &ClickEvent, _, cx| cx.open_url(DUMP_GUIDE_URL)),
                        ),
                ),
            )
    }

    fn enhanced_details_card(&self) -> Div {
        let installed = self.flag("/enhanced/installed");
        let backup = {
            let backup = self.string("/enhanced/backupStatus");
            if backup.is_empty() {
                "Not created".to_owned()
            } else {
                backup
            }
        };
        let game_dir = self.string("/gameDir");
        let install_path = if installed && !game_dir.is_empty() {
            game_dir
        } else {
            "Not created".to_owned()
        };
        let last_install = format_timestamp(&self.string("/enhanced/detectedAt"));
        let last_validation = self
            .validation
            .checked_at
            .clone()
            .unwrap_or_else(|| "Never".into());
        let cells = [
            ("Last install", last_install),
            ("Last validation", last_validation),
            (
                "Files installed",
                self.int("/enhanced/filesInstalled", 0).to_string(),
            ),
            ("Backup status", backup.clone()),
            ("Install path", install_path),
            (
                "Launch target",
                if self.flag("/enhanced/launchOptionsActive") {
                    "BO3 Enhanced"
                } else {
                    "Stock BO3"
                }
                .to_owned(),
            ),
            (
                "Config file",
                if installed { "Configured" } else { "Missing" }.to_owned(),
            ),
            (
                "Restore point",
                if backup == "Created" {
                    "Available"
                } else {
                    "Not available"
                }
                .to_owned(),
            ),
        ];
        card()
            .child(card_title(None, "Install Details / Diagnostics"))
            .child(div().flex().flex_wrap().gap(px(14.)).children(
                cells.into_iter().map(|(label, value)| {
                    metric(label, value, theme::muted_strong()).min_w(px(180.))
                }),
            ))
    }

    fn enhanced_danger_zone(&self, cx: &mut Context<Self>) -> Div {
        let open = self.danger_open;
        card()
            .border_color(theme::accent_alpha(0.34))
            .bg(theme::accent_alpha(0.035))
            .child(
                div()
                    .id("danger-zone")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .font_weight(FontWeight::BOLD)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.danger_open = !view.danger_open;
                        cx.notify();
                    }))
                    .child(icon(
                        if open {
                            Glyph::ChevronDown
                        } else {
                            Glyph::ChevronRight
                        },
                        15.,
                        theme::muted(),
                    ))
                    .child("Danger Zone"),
            )
            .when(open, |this| {
                this.child(
                    div().flex().justify_end().child(
                        self.action(
                            cx,
                            "uninstall-enhanced",
                            "Uninstall BO3 Enhanced",
                            "/api/enhanced-uninstall",
                            json!({}),
                        )
                        .icon(Glyph::Trash)
                        .danger()
                        .disabled(!self.can_act() || !self.flag("/enhanced/installed")),
                    ),
                )
            })
    }

    pub(super) fn enhanced_view(&self, cx: &mut Context<Self>) -> AnyElement {
        bare_panel(
            "Enhanced",
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(self.enhanced_status_card())
                .child(
                    columns()
                        .items_start()
                        .child(column(420.).child(self.enhanced_source_card(cx)))
                        .child(column(280.).child(self.enhanced_help_card(cx))),
                )
                .child(self.enhanced_details_card())
                .child(self.enhanced_danger_zone(cx)),
        )
        .into_any_element()
    }
}
