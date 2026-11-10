//! Enhanced tab (legacy `activeView === "enhanced"`, main.tsx ~L1483-1600 plus the handlers
//! `validateEnhancedSource` / `useDroppedEnhancedSource` / `selectBrowsePath("dump")`).
//!
//! * Status card: Installed / Launch override / Game / Tracking metrics.
//! * Install Source card: `DropZone`, Browse (`BrowseTarget::DumpFolder`, answered through
//!   `ControllerEvent::DirSelected`), Validate (`Action::EnhancedValidate`, answered through
//!   `ControllerEvent::Finished` with `ReplyData::Validated`), source + validation metrics,
//!   Install (`Action::EnhancedInstall`) with download progress.
//! * Help / Requirements card with the dump guide link (`Action::OpenUrl`).
//! * Install Details / Diagnostics grid, and the Danger Zone `Collapsible` with a confirmed
//!   Uninstall.
//!
//! Local view state: the dump source text, the last validation result and the Danger Zone open
//! flag. The source is seeded from `state.enhanced.dump_source` and re-seeded whenever that value
//! changes (legacy `useEffect` on `state.enhanced.dumpSource`).

use gpui::{div, prelude::*, px, Context, Entity, FontWeight, IntoElement, Render, SharedString, Subscription, Window};

use patchops_core::clock;

use crate::actions::{Action, ReplyData};
use crate::controller::{BrowseTarget, Controller, ControllerEvent};
use crate::theme::{ActiveTheme, Theme};
use crate::views::{self, PANEL_GAP};
use crate::widgets::{icon, Button, Card, Collapsible, DropZone, Icon, ProgressBar, ScrollArea, Tone};

const GUIDE_URL: &str = "https://youtu.be/rBZZTcSJ9_s?si=41p0r_Enten3h5AQ";

/// Legacy `enhancedValidation` state: label, tri-state result and the local check time.
#[derive(Clone, Debug)]
struct Validation {
    label: String,
    ok: Option<bool>,
    checked_at: Option<String>,
}

impl Validation {
    fn not_run() -> Self {
        Self { label: "Not run".into(), ok: None, checked_at: None }
    }
    fn failed() -> Self {
        Self { label: "Validation failed".into(), ok: Some(false), checked_at: Some(clock::now().format()) }
    }
    /// Legacy `enhancedValidationLabel`: the message plus the check time when there is one.
    fn display(&self) -> String {
        match &self.checked_at {
            Some(ts) => format!("{} ({ts})", self.label),
            None => self.label.clone(),
        }
    }
    fn tone(&self) -> Tone {
        match self.ok {
            Some(true) => Tone::Ok,
            Some(false) => Tone::Warn,
            None => Tone::Neutral,
        }
    }
}

pub struct EnhancedView {
    c: Entity<Controller>,
    /// Dump source path (legacy `enhancedDumpSource`).
    source: String,
    /// The `state.enhanced.dump_source` value `source` was last seeded from.
    synced_source: Option<String>,
    validation: Validation,
    /// Danger Zone `<details>` open flag.
    danger_open: bool,
    _subs: Vec<Subscription>,
}

impl EnhancedView {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        let initial = controller.read(cx).state().map(|s| s.enhanced.dump_source.clone());
        let observe = cx.observe(&controller, |this, c, cx| {
            if let Some(saved) = c.read(cx).state().map(|s| s.enhanced.dump_source.clone()) {
                if this.synced_source.as_deref() != Some(saved.as_str()) {
                    this.source = saved.clone();
                    this.synced_source = Some(saved);
                }
            }
            cx.notify();
        });
        let finished = cx.subscribe(&controller, |this, _, ev: &ControllerEvent, cx| {
            match ev {
                ControllerEvent::Finished(f) if f.key == "enhanced-validate" => {
                    this.validation = match &f.data {
                        _ if !f.ok => Validation::failed(),
                        ReplyData::Validated { valid, message } => {
                            let label = if message.is_empty() {
                                if *valid { "Ready" } else { "Not valid" }.to_string()
                            } else {
                                message.clone()
                            };
                            Validation { label, ok: Some(*valid), checked_at: Some(clock::now().format()) }
                        }
                        _ => Validation::failed(),
                    };
                    cx.notify();
                }
                ControllerEvent::DirSelected { target: BrowseTarget::DumpFolder, path } => {
                    this.source = path.clone();
                    this.validation = Validation::not_run();
                    cx.notify();
                }
                _ => {}
            }
        });
        Self {
            c: controller,
            source: initial.clone().unwrap_or_default(),
            synced_source: initial,
            validation: Validation::not_run(),
            danger_open: false,
            _subs: vec![observe, finished],
        }
    }
}

/// Legacy `.enhanced-metric`: label above a bold value.
fn stat(t: &Theme, label: &'static str, value: impl Into<SharedString>, tone: Tone) -> impl IntoElement {
    let color = if tone == Tone::Neutral { t.text_strong } else { tone.color(t) };
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .min_w_0()
        .child(
            div()
                .text_size(px(t.font_xs))
                .font_weight(FontWeight::BOLD)
                .text_color(t.text_muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(t.font_sm))
                .font_weight(FontWeight::BOLD)
                .text_color(color)
                .child(value.into()),
        )
}

/// Legacy `.requirement-line`: fixed glyph, green when satisfied, orange otherwise.
fn requirement_line(t: &Theme, label: &'static str, ok: bool, glyph: Icon) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(t.font_sm))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text_strong)
        .child(icon(glyph).size(px(15.)).text_color(if ok { t.ok } else { t.warning }))
        .child(label)
}

/// Legacy `.enhanced-help-summary` line: bold key, muted value.
fn summary_line(t: &Theme, key: &'static str, value: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .gap(px(4.))
        .text_size(px(t.font_xs))
        .text_color(t.text_strong)
        .child(div().font_weight(FontWeight::BOLD).text_color(t.text).child(key))
        .child(value)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// Inverse of [`days_from_civil`].
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Seconds the local clock is ahead of UTC right now (`clock` only knows "now" in local time, so
/// the offset is the difference between its broken-down time and the UTC epoch; accurate to the
/// current DST rule, which is what a timestamp from the same season needs).
fn local_utc_offset_secs() -> i64 {
    let l = clock::now();
    let local = days_from_civil(i64::from(l.year), i64::from(l.month), i64::from(l.day)) * 86_400
        + i64::from(l.hour) * 3600
        + i64::from(l.minute) * 60
        + i64::from(l.second);
    let utc = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(local);
    // Round to the nearest minute: the two clock reads are not atomic.
    ((local - utc) as f64 / 60.0).round() as i64 * 60
}

/// `state.enhanced.detected_at` is a UTC ISO string (`2026-11-10T14:03:22Z`); legacy showed it with
/// `toLocaleString()`, so show local time as `YYYY-MM-DD HH:MM:SS` (same format as the validation time).
fn local_display(raw: Option<&str>) -> String {
    let Some(raw) = raw.filter(|s| !s.is_empty()) else { return "Never".into() };
    let parsed = (|| {
        let (date, time) = raw.trim_end_matches('Z').split(['T', ' ']).filter(|p| !p.is_empty()).fold(
            (None, None),
            |(d, t), part| if d.is_none() { (Some(part), t) } else { (d, t.or(Some(part))) },
        );
        let mut dp = date?.split('-').map(|n| n.parse::<i64>().ok());
        let (y, m, d) = (dp.next()??, dp.next()??, dp.next()??);
        let time = time.unwrap_or("00:00:00").split(['.', '+']).next().unwrap_or("00:00:00");
        let mut tp = time.split(':').map(|n| n.parse::<i64>().ok());
        let (hh, mm, ss) = (tp.next()??, tp.next().flatten().unwrap_or(0), tp.next().flatten().unwrap_or(0));
        Some(days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + ss)
    })();
    match parsed {
        Some(utc) => {
            let t = utc + local_utc_offset_secs();
            let (y, m, d) = civil_from_days(t.div_euclid(86_400));
            let sod = t.rem_euclid(86_400);
            format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", sod / 3600, sod / 60 % 60, sod % 60)
        }
        None => raw.to_string(),
    }
}

impl Render for EnhancedView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let cw = views::content_width(window);
        let cols: u16 = if cw >= 900. { 4 } else if cw >= 480. { 2 } else { 1 };
        let narrow = cw < 800.;
        let ctl = self.c.read(cx);
        let Some(state) = ctl.state().cloned() else {
            return div().into_any_element();
        };
        let busy = |k: &'static str| ctl.is_busy(k);
        let progress = ctl.progress("enhanced-install");
        let c = self.c.clone();
        let this = cx.entity();
        let e = &state.enhanced;

        // ---- Status -----------------------------------------------------------------------
        let detected = e.detected_at.as_deref().is_some_and(|s| !s.is_empty());
        let status = Card::new().icon(Icon::Gem).title("BO3 Enhanced Status").child(
            div()
                .grid()
                .grid_cols(cols)
                .gap(px(14.))
                .child(stat(&t, "Status", if e.installed { "Installed" } else { "Not installed" }, Tone::from_ok(e.installed)))
                .child(stat(
                    &t,
                    "Launch override",
                    if e.launch_options_active { "Active" } else { "Inactive" },
                    Tone::from_ok(e.launch_options_active),
                ))
                .child(stat(
                    &t,
                    "Game",
                    if state.game_detected { "Detected" } else { "Not detected" },
                    Tone::from_ok(state.game_detected),
                ))
                .child(stat(&t, "Tracking", if detected { "Tracked" } else { "Not tracked" }, Tone::from_ok(detected))),
        );

        // ---- Install Source ---------------------------------------------------------------
        let source_trimmed = self.source.trim().to_string();
        let has_source = !source_trimmed.is_empty();
        let valid = self.validation.ok == Some(true);
        let installing = busy("enhanced-install");
        let validating = busy("enhanced-validate");
        let source_label: String = if has_source { source_trimmed.clone() } else { "None".into() };

        let drop = {
            let this = this.clone();
            DropZone::new("enhanced-drop", "Drop DUMP.zip or extracted folder here")
                .detail("or browse to the dump source manually")
                .on_drop(move |paths, _, cx| {
                    let Some(first) = paths.first() else { return };
                    let next = first.to_string_lossy().into_owned();
                    this.update(cx, |v, cx| {
                        v.source = next;
                        v.validation = Validation::not_run();
                        cx.notify();
                    });
                })
        };

        let browse = Button::new("enhanced-browse", "Browse").icon(Icon::FolderOpen).on_click(cx.listener(
            |this, _, _, cx| {
                let start = if this.source.trim().is_empty() {
                    this.c.read(cx).state().and_then(|s| s.game_dir.clone())
                } else {
                    Some(this.source.clone())
                };
                this.c.update(cx, |c, cx| c.open_browser(BrowseTarget::DumpFolder, start, cx));
            },
        ));
        let validate = Button::new("enhanced-validate", "Validate Source")
            .icon(Icon::CircleCheck)
            .disabled(!has_source || validating)
            .on_click(cx.listener(|this, _, _, cx| {
                // Read the source at click time (a drop / browse result may have landed after render).
                let source = this.source.trim().to_string();
                this.c.update(cx, |c, cx| c.run(Action::EnhancedValidate(source), cx));
            }));

        let meta = div()
            .flex()
            .flex_wrap()
            .gap(px(12.))
            .child(div().flex_1().min_w(px(150.)).child(stat(&t, "Source", source_label, Tone::Neutral)))
            .child(div().flex_1().min_w(px(150.)).child(stat(
                &t,
                "Validation",
                self.validation.display(),
                self.validation.tone(),
            )));

        let install = Button::new("enhanced-install", "Install / Update BO3 Enhanced")
            .primary()
            .icon(Icon::Download)
            .full_width()
            .disabled(!has_source || !valid)
            .busy(installing)
            .on_click(cx.listener(|this, _, _, cx| {
                let source = this.source.clone();
                this.c.update(cx, |c, cx| c.run(Action::EnhancedInstall(source), cx));
            }));

        let controls = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .min_w_0()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(10.))
                    .child(div().flex_1().min_w(px(150.)).child(browse.full_width()))
                    .child(div().flex_1().min_w(px(170.)).child(validate.full_width())),
            )
            .child(meta)
            .child(install)
            .when(installing, |d| {
                // Download percent; indeterminate while the size is unknown.
                d.child(ProgressBar::new("enhanced-progress").value(progress.map(|p| f32::from(p) / 100.)))
            })
            .when(!valid, |d| {
                d.child(
                    div()
                        .text_size(px(t.font_xs))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.text_muted)
                        .child("Install requires a valid DUMP.zip or extracted UWP dump folder."),
                )
            });

        let source_card = Card::new().icon(Icon::Download).title("Install Source").child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(12.))
                .child(div().flex_1().min_w(px(240.)).child(drop))
                .child(div().flex_1().min_w(px(340.)).child(controls)),
        );

        // ---- Help / Requirements ----------------------------------------------------------
        let help = Card::new()
            .icon(Icon::TriangleAlert)
            .title("Help / Requirements")
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .child(requirement_line(&t, "Game detected", state.game_detected, Icon::CircleCheck))
                    .child(requirement_line(&t, "Dump required", valid, Icon::TriangleAlert)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(summary_line(&t, "Sources:", " DUMP.zip or extracted folder"))
                    .child(summary_line(&t, "Checks:", " files, read/write, version")),
            )
            .child(
                div().flex().child(
                    Button::new("enhanced-guide", "Open Dump Guide")
                        .icon(Icon::ExternalLink)
                        .on_click(views::run(&c, Action::OpenUrl(GUIDE_URL.into()))),
                ),
            );

        let main = div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap(px(PANEL_GAP))
            .child(div().flex_1().min_w(px(420.)).child(source_card))
            .child(div().flex_none().when(narrow, |d| d.w_full()).when(!narrow, |d| d.w(px(300.))).child(help));

        // ---- Install Details / Diagnostics ------------------------------------------------
        let install_path = match state.game_dir.as_deref().filter(|s| !s.is_empty()) {
            Some(dir) if e.installed => dir.to_string(),
            _ => "Not created".into(),
        };
        let details = Card::new().title("Install Details / Diagnostics").child(
            div()
                .grid()
                .grid_cols(cols)
                .gap(px(14.))
                .child(stat(&t, "Last install", local_display(e.detected_at.as_deref()), Tone::Neutral))
                .child(stat(
                    &t,
                    "Last validation",
                    self.validation.checked_at.clone().unwrap_or_else(|| "Never".into()),
                    Tone::Neutral,
                ))
                .child(stat(&t, "Files installed", e.files_installed.to_string(), Tone::Neutral))
                .child(stat(&t, "Backup status", e.backup_status.clone(), Tone::Neutral))
                .child(stat(&t, "Install path", install_path, Tone::Neutral))
                .child(stat(
                    &t,
                    "Launch target",
                    if e.launch_options_active { "BO3 Enhanced" } else { "Stock BO3" },
                    Tone::Neutral,
                ))
                .child(stat(&t, "Config file", if e.installed { "Configured" } else { "Missing" }, Tone::Neutral))
                .child(stat(
                    &t,
                    "Restore point",
                    if e.backup_status == "Created" { "Available" } else { "Not available" },
                    Tone::Neutral,
                )),
        );

        // ---- Danger Zone ------------------------------------------------------------------
        let danger = Collapsible::new("enhanced-danger", "Danger Zone", self.danger_open)
            .on_toggle({
                let this = this.clone();
                move |open, _, cx| {
                    this.update(cx, |v, cx| {
                        v.danger_open = open;
                        cx.notify();
                    });
                }
            })
            .child(
                div().flex().justify_end().child(
                    Button::new("enhanced-uninstall", "Uninstall BO3 Enhanced")
                        .danger()
                        .icon(Icon::Trash2)
                        .disabled(!e.installed)
                        .busy(busy("enhanced-uninstall"))
                        .on_click(views::run_confirmed(&c, Action::EnhancedUninstall)),
                ),
            );

        ScrollArea::new("enhanced-scroll")
            .gap(px(PANEL_GAP))
            .child(
                div()
                    .flex_none()
                    .text_size(px(t.font_lg))
                    .font_weight(FontWeight::BOLD)
                    .text_color(t.text)
                    .child("Enhanced"),
            )
            .child(div().flex_none().child(status))
            .child(div().flex_none().child(main))
            .child(div().flex_none().child(details))
            .child(div().flex_none().child(danger))
            .into_any_element()
    }
}
