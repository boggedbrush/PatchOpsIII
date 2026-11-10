//! Tools tab (legacy `activeView === "tools"`, main.tsx ~L1771-1850). Six cards in a two-column
//! grid: System, Updates, Mod Cache, Cache Actions, Logs, Log Actions.
//!
//! Port notes (kept deliberately close to the legacy markup):
//! * the title is drawn without the bordered body, because `.tools-panel .panel-body` has no
//!   border or background in the legacy CSS;
//! * legacy buttons only go inert while busy (no spinner), so they use `.disabled(..)`;
//! * a metric's value sits right after its label (legacy `.tool-metric` grid), not at the
//!   far edge like the kit's `Metric`; green values use the legacy `#24ff66`.
//! * the confirm texts (Clear Mod Files, Reset to Stock), the clipboard write for Copy Logs and
//!   the update-check result all come from the controller; this view only dispatches.

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, Div, Entity, FontWeight, Hsla, IntoElement, Render, Subscription, Window};

use crate::actions::{Action, Channel};
use crate::controller::Controller;
use crate::theme::{ActiveTheme, Theme};
use crate::views::{self, PANEL_GAP};
use crate::widgets::{icon, Button, Icon, ScrollArea, Segmented};

/// Legacy `.tool-metric strong.ok` colour (brighter than the theme's `ok`).
const METRIC_OK: u32 = 0x24ff66;

pub struct ToolsView {
    c: Entity<Controller>,
    _sub: Subscription,
}

impl ToolsView {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        let _sub = views::watch(&controller, cx);
        Self { c: controller, _sub }
    }
}

/// Legacy `ToolCard` (`.tool-card`): bordered box, min height 154px, icon + title, then `body`.
fn tool_card(t: Theme, title: &'static str, glyph: Icon, body: impl IntoIterator<Item = AnyElement>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .min_w_0()
        .min_h(px(154.))
        .p(px(14.))
        .border_1()
        .border_color(t.border)
        .rounded(px(t.radius - 2.))
        .bg(gpui::white().opacity(0.025))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(9.))
                .text_size(px(t.font_md))
                .text_color(t.text)
                .child(icon(glyph).text_color(t.text_muted))
                .child(title),
        )
        .children(body)
}

/// Column of `ToolMetric` rows (legacy `.tool-metric-list`).
fn metric_list() -> Div {
    div().flex().flex_col().gap(px(8.)).min_w_0()
}

/// A metric whose value is a path: one line showing the END of the path (the start is replaced by
/// an ellipsis, like the directory browser's header), the full path in a tooltip.
/// `value_px` is the width available to the value.
fn metric_path(t: Theme, label: &'static str, path: &str, label_w: f32, value_px: f32) -> impl IntoElement {
    let max_chars = ((value_px / (t.font_sm * 0.6)).floor() as usize).max(12);
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .min_w_0()
        .text_size(px(t.font_sm))
        .text_color(t.text_muted)
        .child(div().flex_none().min_w(px(label_w)).font_weight(FontWeight::BOLD).child(label))
        .child(
            div()
                .id("metric-path")
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .font_weight(FontWeight::BOLD)
                .text_color(t.text)
                .tooltip(crate::widgets::tooltip(path.to_string()))
                .child(crate::widgets::dir_browser::shorten_start(path, max_chars)),
        )
}

/// Label column width of the metric rows (legacy `.tool-metric`).
const LABEL_W: f32 = 90.;
/// Card chrome around a metric row: padding + border + the label/value gap.
const CARD_CHROME: f32 = 2. * 14. + 2. + 10.;

/// Legacy `ToolMetric`: label column (at least `label_w` px) then the value, green when `ok`.
fn metric(t: Theme, label: &'static str, value: String, ok: bool, label_w: f32) -> impl IntoElement {
    let value_color: Hsla = if ok { rgb(METRIC_OK).into() } else { t.text };
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .min_w_0()
        .text_size(px(t.font_sm))
        .text_color(t.text_muted)
        .child(div().flex_none().min_w(px(label_w)).font_weight(FontWeight::BOLD).child(label))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_weight(FontWeight::BOLD)
                .text_color(value_color)
                .child(value),
        )
}

impl Render for ToolsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let cw = views::content_width(window);
        let two_cols = cw >= 760.;
        let card_w = if two_cols { (cw - PANEL_GAP) / 2. } else { cw };
        let ctl = self.c.read(cx);
        let Some(state) = ctl.state().cloned() else {
            return div().into_any_element();
        };
        let busy = |k: &'static str| ctl.is_busy(k);
        let ready = ctl.ready();
        let latest_error = ctl.last_error_log().is_some();
        let originals = ctl.originals_dir().to_string_lossy().into_owned();
        let entries = ctl.log_count(cx);
        let c = self.c.clone();

        // ---- System -----------------------------------------------------------------------
        let platform_ok = !state.platform.is_empty();
        let steam_id = state.steam_user_id.clone().filter(|s| !s.is_empty());
        let system = tool_card(
            t,
            "System",
            Icon::Cpu,
            [metric_list()
                .child(metric(
                    t,
                    "Platform",
                    if platform_ok { state.platform.clone() } else { "Unknown".into() },
                    platform_ok,
                    90.,
                ))
                .child(metric(
                    t,
                    "Steam ID",
                    steam_id.clone().unwrap_or_else(|| "Not found".into()),
                    steam_id.is_some(),
                    90.,
                ))
                .child(metric(t, "PatchOps", state.app_version.clone(), true, 90.))
                .into_any_element()],
        );

        // ---- Updates ----------------------------------------------------------------------
        let is_beta = state.release_channel == "beta";
        let channel = Segmented::new("release-channel", ["Stable", "Beta"])
            .selected(if is_beta { 1 } else { 0 })
            .disabled(busy("release-channel"))
            .on_change({
                let c = c.clone();
                move |ix, _w, cx| {
                    let channel = if ix == 0 { Channel::Stable } else { Channel::Beta };
                    c.update(cx, |ctl, cx| ctl.run(Action::SetReleaseChannel(channel), cx));
                }
            });
        let current = if is_beta { "Beta" } else { "Stable" };
        let updates = tool_card(
            t,
            "Updates",
            Icon::RefreshCw,
            [
                div().w_full().min_w_0().child(channel).into_any_element(),
                div()
                    .flex()
                    .flex_wrap()
                    .gap_x(px(8.))
                    .gap_y(px(8.))
                    .min_w_0()
                    .child(div().flex_1().min_w(px(170.)).child(metric(t, "Current", current.into(), true, 0.)))
                    .child(div().flex_1().min_w(px(190.)).child(metric(t, "Last checked", "On demand".into(), false, 0.)))
                    .into_any_element(),
                Button::new("update-check", "Check for Updates")
                    .small()
                    .full_width()
                    .icon(Icon::RefreshCw)
                    .disabled(!ready || busy("update-check"))
                    .on_click(views::run(&c, Action::UpdateCheck))
                    .into_any_element(),
            ],
        );

        // ---- Mod Cache --------------------------------------------------------------------
        let mod_dir_ok = !state.maintenance.mod_files_dir.is_empty();
        let mod_cache = tool_card(
            t,
            "Mod Cache",
            Icon::Download,
            [metric_list()
                .child(metric(
                    t,
                    "Status",
                    if mod_dir_ok { "Configured".into() } else { "Missing".into() },
                    mod_dir_ok,
                    90.,
                ))
                .child(metric(
                    t,
                    "Directory",
                    if mod_dir_ok { "Ready".into() } else { "Not found".into() },
                    mod_dir_ok,
                    90.,
                ))
                .child(metric_path(t, "Backups", &originals, LABEL_W, card_w - CARD_CHROME - LABEL_W))
                .into_any_element()],
        );

        // ---- Cache Actions ----------------------------------------------------------------
        let cache_actions = tool_card(
            t,
            "Cache Actions",
            Icon::Trash2,
            [div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .min_w_0()
                .child(
                    div().flex_1().min_w(px(150.)).child(
                        Button::new("clear-mod-files", "Clear Mod Files")
                            .small()
                            .full_width()
                            .icon(Icon::Trash2)
                            .disabled(busy("clear-mod-files"))
                            .on_click(views::run_confirmed(&c, Action::ClearModFiles)),
                    ),
                )
                .child(
                    div().flex_1().min_w(px(150.)).child(
                        Button::new("reset-stock", "Reset to Stock")
                            .small()
                            .full_width()
                            .danger()
                            .icon(Icon::RotateCcw)
                            .disabled(!state.game_detected || busy("reset-stock"))
                            .on_click(views::run_confirmed(&c, Action::ResetStock)),
                    ),
                )
                .into_any_element()],
        );

        // ---- Logs -------------------------------------------------------------------------
        let log_file_ok = !state.log_path.is_empty();
        let logs = tool_card(
            t,
            "Logs",
            Icon::Clipboard,
            [metric_list()
                .child(metric(
                    t,
                    "Log file",
                    if log_file_ok { "Available".into() } else { "Missing".into() },
                    log_file_ok,
                    90.,
                ))
                .child(metric(
                    t,
                    "Last error",
                    if latest_error { "See Activity Log".into() } else { "None".into() },
                    !latest_error,
                    90.,
                ))
                .child(metric(t, "Entries", entries.to_string(), entries > 0, 90.))
                .into_any_element()],
        );

        // ---- Log Actions ------------------------------------------------------------------
        let log_actions = tool_card(
            t,
            "Log Actions",
            Icon::Eraser,
            [div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .min_w_0()
                .child(
                    div().flex_1().min_w(px(150.)).child(
                        Button::new("copy-logs", "Copy Logs")
                            .small()
                            .full_width()
                            .icon(Icon::Clipboard)
                            .disabled(busy("copy-logs"))
                            .on_click(views::run(&c, Action::CopyLogs)),
                    ),
                )
                .child(
                    div().flex_1().min_w(px(150.)).child(
                        Button::new("clear-logs", "Clear Logs")
                            .small()
                            .full_width()
                            .icon(Icon::Eraser)
                            .disabled(busy("clear-logs"))
                            .on_click(views::run(&c, Action::ClearLogs)),
                    ),
                )
                .into_any_element()],
        );

        // Legacy `.tools-grid`: two columns, cards aligned to the top, 14px padding.
        let grid = div()
            .grid()
            .grid_cols(if two_cols { 2 } else { 1 })
            .gap(px(13.))
            .items_start()
            .p(px(14.))
            .child(system)
            .child(updates)
            .child(mod_cache)
            .child(cache_actions)
            .child(logs)
            .child(log_actions);

        ScrollArea::new("tools-scroll")
            .gap(px(PANEL_GAP))
            .child(div().flex().flex_none().flex_col().child(views::heading(cx, "Tools")).child(grid))
            .into_any_element()
    }
}
