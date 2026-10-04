//! Tools: system information, updates, caches and logs.
use super::components::{
    Btn, Glyph, bare_panel, card, card_title, columns, handler, segmented, tool_metric,
};
use super::progress::Operation;
use super::{ControlCenter, theme};
use gpui::{prelude::*, *};
use serde_json::json;

impl ControlCenter {
    fn tool_card(&self, glyph: Glyph, title: &str) -> Div {
        card()
            .flex_1()
            .min_w(px(300.))
            .min_h(px(154.))
            .child(card_title(Some(glyph), title.to_owned()))
    }

    fn copy_logs(&mut self, cx: &mut Context<Self>) {
        let payload = self.string("/maintenance/logPayload");
        let payload = if payload.is_empty() {
            self.visible_logs()
                .into_iter()
                .map(|(category, message)| format!("[{category}] {message}"))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            payload
        };
        if payload.is_empty() {
            self.reject("There are no log entries to copy.", cx);
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(payload));
        self.failed = false;
        self.message = "Copied the logs to the clipboard.".into();
        cx.notify();
    }

    pub(super) fn tools_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let can_act = self.can_act();
        let channel = self.string("/releaseChannel");
        let channel_label = if channel == "beta" { "Beta" } else { "Stable" };
        let platform = self.string("/platform");
        let steam_id = self.string("/steamUserId");
        let mod_files = self.string("/maintenance/modFilesDir");
        let log_path = self.string("/logPath");
        let entries = self.visible_logs();
        let last_error = entries.iter().any(|(category, _)| category == "Error");
        let channels = segmented(
            [("stable", "Stable"), ("beta", "Beta")]
                .into_iter()
                .map(|(id, label)| {
                    (
                        SharedString::from(label),
                        channel == id,
                        handler(self.stage_click(
                            cx,
                            format!("Use the {label} release channel"),
                            "/api/release-channel",
                            json!({"channel": id}),
                        )),
                    )
                })
                .collect(),
            can_act,
        );
        let system = self
            .tool_card(Glyph::Cpu, "System")
            .child(tool_metric(
                "Platform",
                if platform.is_empty() {
                    "Unknown".to_owned()
                } else {
                    platform.clone()
                },
                !platform.is_empty(),
            ))
            .child(tool_metric(
                "Steam ID",
                if steam_id.is_empty() {
                    "Not found".to_owned()
                } else {
                    steam_id.clone()
                },
                !steam_id.is_empty(),
            ))
            .child(tool_metric("PatchOps", self.value("/appVersion"), true));
        let updates = self
            .tool_card(Glyph::Refresh, "Updates")
            .child(channels)
            .child(tool_metric("Current", channel_label, true))
            .child(tool_metric("Last checked", "On demand", false))
            .child(
                self.action(
                    cx,
                    "check-updates",
                    "Check for Updates",
                    "/api/update-check",
                    json!({}),
                )
                .compact()
                .icon(Glyph::Refresh),
            )
            .children(self.progress_strip(&[Operation::UpdateCheck]));
        let cache = self
            .tool_card(Glyph::Download, "Mod Cache")
            .child(tool_metric(
                "Status",
                if mod_files.is_empty() {
                    "Missing"
                } else {
                    "Configured"
                },
                !mod_files.is_empty(),
            ))
            .child(tool_metric(
                "Directory",
                if mod_files.is_empty() {
                    "Not found"
                } else {
                    "Ready"
                },
                !mod_files.is_empty(),
            ));
        let cache_actions = self
            .tool_card(Glyph::Trash, "Cache Actions")
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        self.action(
                            cx,
                            "clear-mod-files",
                            "Clear Mod Files",
                            "/api/mod-files/clear",
                            json!({}),
                        )
                        .compact()
                        .icon(Glyph::Trash)
                        .grow(),
                    )
                    .child(
                        self.action(
                            cx,
                            "reset-stock",
                            "Reset to Stock",
                            "/api/reset-stock",
                            json!({}),
                        )
                        .compact()
                        .icon(Glyph::Rotate)
                        .danger()
                        .grow()
                        .disabled(!can_act || !self.flag("/gameDetected")),
                    ),
            )
            .children(self.progress_strip(&[Operation::ClearModFiles, Operation::ResetStock]));
        let logs = self
            .tool_card(Glyph::Clipboard, "Logs")
            .child(tool_metric(
                "Log file",
                if log_path.is_empty() {
                    "Missing"
                } else {
                    "Available"
                },
                !log_path.is_empty(),
            ))
            .child(tool_metric(
                "Last error",
                if last_error {
                    "See Activity Log"
                } else {
                    "None"
                },
                !last_error,
            ))
            .child(tool_metric(
                "Entries",
                entries.len().to_string(),
                !entries.is_empty(),
            ))
            .when(!log_path.is_empty(), |this| {
                this.child(
                    div()
                        .truncate()
                        .text_size(px(theme::FONT_XS))
                        .text_color(theme::muted())
                        .child(log_path.clone()),
                )
            });
        let log_actions = self.tool_card(Glyph::Eraser, "Log Actions").child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .child(
                    Btn::new("copy-logs", "Copy Logs")
                        .compact()
                        .icon(Glyph::Clipboard)
                        .grow()
                        .disabled(!self.connected)
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.copy_logs(cx))),
                )
                .child(
                    self.action(cx, "clear-logs", "Clear Logs", "/api/logs/clear", json!({}))
                        .compact()
                        .icon(Glyph::Eraser)
                        .grow(),
                ),
        );
        bare_panel(
            "Tools",
            div()
                .flex()
                .flex_col()
                .gap(px(theme::PANEL_GAP))
                .child(columns().child(system).child(updates))
                .child(columns().child(cache).child(cache_actions))
                .child(columns().child(logs).child(log_actions)),
        )
        .into_any_element()
    }
}
