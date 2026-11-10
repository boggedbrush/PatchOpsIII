//! EXE Swapper tab (legacy `activeView === "exe"`, main.tsx ~L1395-1481): an active-build hero
//! (title + three status pills) and three option cards (Compatible, Latest, BO3 Enhanced). Each
//! card has a badge, pro / warn / con lines and its action button.
//!
//! The depot dialog for the compatible build is owned by the controller: `Action::ExeCompatible`
//! opens the "Copy this" prompt by itself when the Steam depot is missing, so this view only
//! dispatches. Enabled / disabled rules are the legacy ones (`compatibleDisabled`,
//! `latestDisabled`, `enhancedDisabled`). The legacy card-level `disabled` class is an opacity
//! of 0.72 on the whole card, and the legacy title does not draw the panel frame.

use gpui::{div, prelude::*, px, relative, rgb, Context, Entity, FontWeight, Hsla, IntoElement, Render, Subscription, Window};

use crate::actions::Action;
use crate::controller::Controller;
use crate::theme::{ActiveTheme, Theme};
use crate::views::{self, PANEL_GAP};
use crate::widgets::{icon, ellipsis, Badge, Button, Icon, ScrollArea, StatusPill, Tone};

/// Legacy `.exe-swap-pros .warn svg` colour.
const WARN_ICON: u32 = 0xffb020;

pub struct ExeSwapView {
    c: Entity<Controller>,
    _sub: Subscription,
}

impl ExeSwapView {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        let _sub = views::watch(&controller, cx);
        Self { c: controller, _sub }
    }
}

/// The icon + colour of one pro / warn / con line (legacy `.pro`, `.warn`, `.con`).
#[derive(Clone, Copy)]
enum Line {
    Pro,
    Warn,
    Con,
}

/// Legacy `<span class="pro"><CheckCircle2/><strong>label:</strong> text</span>`.
fn pro_line(t: Theme, kind: Line, label: &'static str, text: &'static str) -> impl IntoElement {
    let (glyph, color): (Icon, Hsla) = match kind {
        Line::Pro => (Icon::CircleCheck, t.ok),
        Line::Warn => (Icon::TriangleAlert, rgb(WARN_ICON).into()),
        Line::Con => (Icon::X, t.accent),
    };
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_w_0()
        .child(icon(glyph).size(px(15.)).flex_none().text_color(color))
        .child(
            div().flex_1().min_w_0().child(
                div()
                    .flex()
                    .flex_wrap()
                    .child(div().font_weight(FontWeight::BOLD).child(label))
                    .child(div().child(format!(" {text}"))),
            ),
        )
}

/// One option card (legacy `.exe-swap-option`). `last` drops the right border; `basis` is its
/// share of the three-column row (legacy `minmax(0,1fr) minmax(0,1fr) minmax(320px,1.22fr)`).
struct OptionCard {
    title: &'static str,
    subtitle: String,
    badge: &'static str,
    active: bool,
    disabled: bool,
    last: bool,
    basis: f32,
    /// Single-column layout: full width, hairline below instead of to the right.
    stacked: bool,
    lines: [(Line, &'static str, &'static str); 3],
    button: Button,
}

impl OptionCard {
    fn into_element(self, t: Theme) -> impl IntoElement {
        // The badge drops under the title instead of squeezing it to a letter per line.
        let header = div()
            .flex()
            .flex_wrap()
            .justify_between()
            .items_start()
            .gap_x(px(12.))
            .gap_y(px(6.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(150.))
                    .child(div().text_size(px(t.font_lg)).text_color(t.text).child(self.title))
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(t.font_sm))
                            .font_weight(FontWeight::BOLD)
                            .text_color(t.text_muted)
                            .child(self.subtitle),
                    ),
            )
            .child(div().flex_none().child(Badge::new(self.badge).tone(if self.active { Tone::Ok } else { Tone::Neutral })));

        let pros = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(t.font_sm))
            .text_color(t.text)
            .children(self.lines.into_iter().map(|(kind, label, text)| pro_line(t, kind, label, text).into_any_element()));

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .min_w_0()
            .when(self.stacked, |d| d.w_full().flex_none())
            .when(!self.stacked, |d| d.flex_basis(relative(self.basis)))
            .p(px(14.))
            .when(!self.last && !self.stacked, |d| d.border_r_1().border_color(t.border))
            .when(!self.last && self.stacked, |d| d.border_b_1().border_color(t.border))
            .when(self.active, |d| d.border_t(px(4.)).border_color(t.ok).bg(t.ok.opacity(0.045)))
            .when(self.disabled, |d| d.opacity(0.72))
            .child(header)
            .child(pros)
            .child(self.button.full_width())
    }
}

impl Render for ExeSwapView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let cw = views::content_width(window);
        let stack_hero = cw < 1000.;
        let stack_cards = cw < 800.;
        let ctl = self.c.read(cx);
        let Some(state) = ctl.state().cloned() else {
            return div().into_any_element();
        };
        let busy = |k: &'static str| ctl.is_busy(k);
        let c = self.c.clone();
        let exe = &state.exe_swap;

        let profile = exe.profile.as_str();
        let trusted = exe.trusted_executable;
        let active_is = |p: &str| profile == p && trusted;

        // ---- Hero: active build + three status pills ---------------------------------------
        let active_title = if trusted && profile == "compatible" {
            format!("Compatible Build ({})", exe.active_build_id)
        } else if trusted && profile == "current" {
            format!("Latest Build ({})", exe.active_build_id)
        } else if trusted && profile == "enhanced" {
            format!("BO3 Enhanced ({})", exe.active_build_id)
        } else {
            "Unverified EXE".to_string()
        };
        let title_box = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .min_w_0()
            .flex_grow()
            .p(px(12.))
            .border_1()
            .border_color(t.border)
            .rounded(px(t.radius - 4.))
            .bg(gpui::white().opacity(0.025))
            .child(icon(Icon::RefreshCw).size(px(20.)).flex_none().text_color(t.accent))
            .child(
                div()
                    .min_w_0()
                    .child(div().text_size(px(t.font_xs)).font_weight(FontWeight::BOLD).text_color(t.text_muted).child("Active build"))
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(17.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(t.text)
                            .child(ellipsis(active_title)),
                    ),
            );

        // Pills wrap rather than shrink: a pill never gets narrower than its text.
        let pill = |label: String, value: String, ok: bool| {
            div().flex_1().min_w(px(190.)).flex().flex_col().child(StatusPill::new(label, value, Tone::from_ok(ok)))
        };
        let pills = div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .min_w_0()
            .flex_grow()
            .child(pill(
                format!("Latest - {}", exe.current_build_date),
                exe.current_build_id.clone(),
                trusted && profile == "current",
            ))
            .child(pill(
                format!("Compatible - {}", exe.compatible_build_date),
                exe.compatible_build_id.clone(),
                exe.compatible_active,
            ))
            .child(pill(
                "Enhanced".to_string(),
                if exe.enhanced_exe_active {
                    "Active".to_string()
                } else if exe.enhanced_available {
                    "Available".to_string()
                } else {
                    "Not found".to_string()
                },
                exe.enhanced_available,
            ));

        let hero = if stack_hero {
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(div().min_w_0().child(title_box))
                .child(div().min_w_0().child(pills))
        } else {
            div()
                .flex()
                .gap(px(10.))
                .child(div().flex_basis(relative(0.35)).flex_shrink().min_w(px(280.)).flex().flex_col().child(title_box))
                .child(div().flex_basis(relative(0.65)).flex_shrink().min_w_0().flex().flex_col().child(pills))
        };

        // ---- Option cards -----------------------------------------------------------------
        let game = state.game_detected;
        let compatible_disabled = !game || profile == "compatible" || busy("exe-compatible");
        let latest_disabled = !game || profile == "current" || !exe.latest_available || busy("exe-current");
        let enhanced_disabled = !game || profile == "enhanced" || !exe.enhanced_available || busy("exe-enhanced");

        let compatible_active = active_is("compatible");
        let compatible = OptionCard {
            title: "Compatible Build",
            subtitle: "March 3, 2023 Steam depot".into(),
            badge: if compatible_active { "Active" } else { "Best for mods" },
            active: compatible_active,
            disabled: compatible_disabled,
            last: false,
            stacked: stack_cards,
            basis: 1.0 / 3.22,
            lines: [
                (Line::Pro, "Vanilla experience:", "Supported"),
                (Line::Pro, "Modded experience:", "Best support"),
                (Line::Warn, "Performance:", "Standard Steam performance"),
            ],
            button: Button::new("exe-compatible", "Use Compatible Build")
                .primary()
                .icon(Icon::Download)
                .disabled(compatible_disabled)
                .on_click(views::run(&c, Action::ExeCompatible)),
        };

        let latest_active = active_is("current");
        let latest = OptionCard {
            title: "Latest Build",
            subtitle: format!("{} Steam build", exe.current_build_date),
            badge: if latest_active { "Active" } else { "Steam default" },
            active: latest_active,
            disabled: latest_disabled,
            last: false,
            stacked: stack_cards,
            basis: 1.0 / 3.22,
            lines: [
                (Line::Pro, "Vanilla experience:", "Best Steam match"),
                (Line::Warn, "Modded experience:", "May or may not work"),
                (Line::Warn, "Performance:", "Standard Steam performance"),
            ],
            button: Button::new("exe-current", "Use Latest Build")
                .icon(Icon::RotateCcw)
                .disabled(latest_disabled)
                .on_click(views::run(&c, Action::ExeCurrent)),
        };

        let enhanced_active = active_is("enhanced");
        let enhanced = OptionCard {
            title: "BO3 Enhanced",
            subtitle: "Windows Store build modded for Steam".into(),
            badge: if enhanced_active {
                "Active"
            } else if exe.enhanced_available {
                "Best performance"
            } else {
                "Not installed"
            },
            active: enhanced_active,
            disabled: enhanced_disabled,
            last: true,
            stacked: stack_cards,
            basis: 1.22 / 3.22,
            lines: [
                (Line::Pro, "Vanilla experience:", "Supported"),
                (Line::Con, "Modded experience:", "Most mods unlikely"),
                (Line::Pro, "Performance:", "Highest performance"),
            ],
            button: Button::new("exe-enhanced", "Use BO3 Enhanced")
                .icon(Icon::Gem)
                .disabled(enhanced_disabled)
                .on_click(views::run(&c, Action::ExeEnhanced)),
        };

        let options = div()
            .flex()
            .when(stack_cards, |d| d.flex_col())
            .min_w_0()
            .overflow_hidden()
            .border_1()
            .border_color(t.border)
            .rounded(px(t.radius))
            .bg(t.panel)
            .child(compatible.into_element(t))
            .child(latest.into_element(t))
            .child(enhanced.into_element(t));

        ScrollArea::new("exe-scroll")
            .gap(px(PANEL_GAP))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .flex_col()
                    .child(views::heading(cx, "EXE Swapper"))
                    .child(div().flex_none().mb(px(12.)).child(hero))
                    .child(options),
            )
            .into_any_element()
    }
}
