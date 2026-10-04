//! EXE Swapper: the three selectable executables and their trade-offs.
use super::components::{Btn, Glyph, bare_panel, columns, icon, status_pill, text_xs};
use super::progress::Operation;
use super::{ControlCenter, theme};
use gpui::{prelude::*, *};
use serde_json::json;

#[derive(Clone, Copy)]
enum Verdict {
    Pro,
    Warn,
    Con,
}

fn verdict_line(verdict: Verdict, topic: &str, detail: &str) -> Div {
    let (glyph, color) = match verdict {
        Verdict::Pro => (Glyph::CheckCircle, theme::ok()),
        Verdict::Warn => (Glyph::Alert, theme::caution()),
        Verdict::Con => (Glyph::Close, theme::accent()),
    };
    // One run of text with a bold lead-in so long details wrap inside the card.
    let lead = format!("{topic}:");
    let bold = HighlightStyle {
        font_weight: Some(FontWeight::BOLD),
        ..Default::default()
    };
    let text = StyledText::new(format!("{lead} {detail}")).with_highlights([(0..lead.len(), bold)]);
    div()
        .flex()
        .items_start()
        .gap(px(8.))
        .min_w_0()
        .text_size(px(theme::FONT_SM))
        .child(div().pt(px(2.)).child(icon(glyph, 15., color)))
        .child(div().flex_1().min_w_0().child(text))
}

struct ExeOption<'a> {
    title: &'a str,
    subtitle: String,
    badge: &'a str,
    active: bool,
    disabled: bool,
    verdicts: [(Verdict, &'a str, &'a str); 3],
}

fn exe_option(option: ExeOption, button: Btn) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .flex_1()
        .min_w(px(240.))
        .p(px(16.))
        .when(option.active, |this| {
            this.border_t_4()
                .border_color(theme::ok())
                .bg(theme::ok_alpha(0.045))
        })
        .when(option.disabled, |this| this.opacity(0.72))
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(theme::FONT_LG))
                                .font_weight(FontWeight::BOLD)
                                .child(option.title.to_owned()),
                        )
                        .child(
                            div()
                                .text_size(px(theme::FONT_SM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::muted())
                                .child(option.subtitle),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .px(px(9.))
                        .py(px(4.))
                        .border_1()
                        .border_color(if option.active {
                            theme::ok_alpha(0.42)
                        } else {
                            theme::border()
                        })
                        .rounded_full()
                        .bg(if option.active {
                            theme::ok_alpha(0.13)
                        } else {
                            theme::white(0.)
                        })
                        .text_size(px(theme::FONT_XS))
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_color(if option.active {
                            theme::ok()
                        } else {
                            theme::muted()
                        })
                        .child(option.badge.to_owned()),
                ),
        )
        .child(
            div().flex().flex_col().gap(px(10.)).children(
                option
                    .verdicts
                    .into_iter()
                    .map(|(verdict, topic, detail)| verdict_line(verdict, topic, detail)),
            ),
        )
        .child(button)
}

impl ControlCenter {
    pub(super) fn exe_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let profile = self.string("/exeSwap/profile");
        let trusted = self.flag("/exeSwap/trustedExecutable");
        let detected = self.flag("/gameDetected");
        let build_id = self.value("/exeSwap/activeBuildId");
        let current_date = self.value("/exeSwap/currentBuildDate");
        let compatible_date = self.value("/exeSwap/compatibleBuildDate");
        let active_title = match (trusted, profile.as_str()) {
            (true, "compatible") => format!("Compatible Build ({build_id})"),
            (true, "current") => format!("Latest Build ({build_id})"),
            (true, "enhanced") => format!("BO3 Enhanced ({build_id})"),
            _ => "Unverified EXE".to_owned(),
        };
        let enhanced_available = self.flag("/exeSwap/enhancedAvailable");
        let enhanced_value = if self.flag("/exeSwap/enhancedExeActive") {
            "Active"
        } else if enhanced_available {
            "Available"
        } else {
            "Not found"
        };
        let compatible_active = profile == "compatible" && trusted;
        let latest_active = profile == "current" && trusted;
        let enhanced_active = profile == "enhanced" && trusted;
        let compatible_disabled = !detected || profile == "compatible";
        let latest_disabled =
            !detected || profile == "current" || !self.flag("/exeSwap/latestAvailable");
        let enhanced_disabled = !detected || profile == "enhanced" || !enhanced_available;
        let can_act = self.can_act();

        let hero = columns()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .flex_none()
                    .w(px(330.))
                    .p(px(14.))
                    .border_1()
                    .border_color(theme::border())
                    .rounded(theme::radius_card())
                    .bg(theme::white(0.025))
                    .child(icon(Glyph::Refresh, 20., theme::accent()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(text_xs("Active build", theme::muted()))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::EXTRA_BOLD)
                                    .child(active_title),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .flex_1()
                    .min_w(px(420.))
                    .child(status_pill(
                        format!("Latest - {current_date}"),
                        self.value("/exeSwap/currentBuildId"),
                        latest_active,
                    ))
                    .child(status_pill(
                        format!("Compatible - {compatible_date}"),
                        self.value("/exeSwap/compatibleBuildId"),
                        self.flag("/exeSwap/compatibleActive"),
                    ))
                    .child(status_pill("Enhanced", enhanced_value, enhanced_available)),
            );

        let options = div()
            .flex()
            .flex_wrap()
            .border_1()
            .border_color(theme::border())
            .rounded(theme::radius())
            .bg(theme::panel())
            .overflow_hidden()
            .child(
                exe_option(
                    ExeOption {
                        title: "Compatible Build",
                        subtitle: "March 3, 2023 Steam depot".into(),
                        badge: if compatible_active {
                            "Active"
                        } else {
                            "Best for mods"
                        },
                        active: compatible_active,
                        disabled: compatible_disabled,
                        verdicts: [
                            (Verdict::Pro, "Vanilla experience", "Supported"),
                            (Verdict::Pro, "Modded experience", "Best support"),
                            (Verdict::Warn, "Performance", "Standard Steam performance"),
                        ],
                    },
                    self.action(
                        cx,
                        "exe-compatible",
                        "Use Compatible Build",
                        "/api/exe-swap/compatible",
                        json!({}),
                    )
                    .icon(Glyph::Download)
                    .primary()
                    .disabled(!can_act || compatible_disabled),
                )
                .border_r_1()
                .border_color(theme::border()),
            )
            .child(
                exe_option(
                    ExeOption {
                        title: "Latest Build",
                        subtitle: format!("{current_date} Steam build"),
                        badge: if latest_active {
                            "Active"
                        } else {
                            "Steam default"
                        },
                        active: latest_active,
                        disabled: latest_disabled,
                        verdicts: [
                            (Verdict::Pro, "Vanilla experience", "Best Steam match"),
                            (Verdict::Warn, "Modded experience", "May or may not work"),
                            (Verdict::Warn, "Performance", "Standard Steam performance"),
                        ],
                    },
                    self.action(
                        cx,
                        "exe-current",
                        "Use Latest Build",
                        "/api/exe-swap/current",
                        json!({}),
                    )
                    .icon(Glyph::Rotate)
                    .disabled(!can_act || latest_disabled),
                )
                .border_r_1()
                .border_color(theme::border()),
            )
            .child(exe_option(
                ExeOption {
                    title: "BO3 Enhanced",
                    subtitle: "Windows Store build modded for Steam".into(),
                    badge: if enhanced_active {
                        "Active"
                    } else if enhanced_available {
                        "Best performance"
                    } else {
                        "Not installed"
                    },
                    active: enhanced_active,
                    disabled: enhanced_disabled,
                    verdicts: [
                        (Verdict::Pro, "Vanilla experience", "Supported"),
                        (Verdict::Con, "Modded experience", "Most mods unlikely"),
                        (Verdict::Pro, "Performance", "Highest performance"),
                    ],
                },
                self.action(
                    cx,
                    "exe-enhanced",
                    "Use BO3 Enhanced",
                    "/api/exe-swap/enhanced",
                    json!({}),
                )
                .icon(Glyph::Gem)
                .disabled(!can_act || enhanced_disabled),
            ));

        let details = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(12.))
            .border_1()
            .border_color(theme::border())
            .rounded(theme::radius_control())
            .bg(theme::scrim().alpha(0.18))
            .text_size(px(theme::FONT_XS))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::muted_strong())
            .child(self.value("/exeSwap/displayLabel"))
            .child(self.value("/exeSwap/integrityMessage"))
            .child(format!("SHA-256: {}", self.value("/exeSwap/executableHash")))
            .child("Compatible installs may need a Steam depot download. The command appears in a dialog when it is required.");

        bare_panel(
            "EXE Swapper",
            div()
                .flex()
                .flex_col()
                .gap(px(theme::PANEL_GAP))
                .child(hero)
                .children(self.progress_strip(&[
                    Operation::ExeCompatible,
                    Operation::ExeCurrent,
                    Operation::ExeEnhanced,
                ]))
                .child(options)
                .child(details),
        )
        .into_any_element()
    }
}
