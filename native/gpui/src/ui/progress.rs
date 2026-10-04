//! Long-running operation feedback: the model behind the progress strips on
//! each page and the entry point the backend's event hook feeds.
//!
//! PR #40 (`migrate/tauri-in-process-rust`) has exactly one backend event,
//! `patchops-log` carrying `{category, message, line}`, and a frontend-only
//! `busy` id per command (`t7-install`, `dxvk-install`, `enhanced-install`,
//! `workshop-install`, ...). It renders no progress bars: buttons are disabled
//! while `busy` is set and log lines are appended as they stream in. This
//! module keeps that contract (`ProgressEvent::Log`, [`Operation::id`]) and
//! adds an optional stage/fraction channel for when the core can report one.
use super::components::{Glyph, icon};
use super::{ControlCenter, theme};
use gpui::{prelude::*, *};
use serde_json::json;
use std::time::{Duration, Instant};

/// How long a finished operation stays visible before its strip disappears.
const OUTCOME_LINGER: Duration = Duration::from_secs(4);
/// Live log lines kept between status snapshots.
const LIVE_LOG_LIMIT: usize = 300;

/// A long-running operation. [`Operation::id`] matches PR #40's `busy` ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    T7Install,
    T7Uninstall,
    DxvkInstall,
    DxvkUninstall,
    EnhancedValidate,
    EnhancedInstall,
    EnhancedUninstall,
    WorkshopInstall,
    ExeCompatible,
    ExeCurrent,
    ExeEnhanced,
    UpdateCheck,
    ResetStock,
    ClearModFiles,
}

impl Operation {
    /// The operation a request path starts, if it is worth a progress strip.
    pub fn from_path(path: &str) -> Option<Self> {
        Some(match path {
            "/api/t7-install" => Self::T7Install,
            "/api/t7-uninstall" => Self::T7Uninstall,
            "/api/dxvk-install" => Self::DxvkInstall,
            "/api/dxvk-uninstall" => Self::DxvkUninstall,
            "/api/enhanced-validate" => Self::EnhancedValidate,
            "/api/enhanced-install" => Self::EnhancedInstall,
            "/api/enhanced-uninstall" => Self::EnhancedUninstall,
            "/api/workshop-install" => Self::WorkshopInstall,
            "/api/exe-swap/compatible" => Self::ExeCompatible,
            "/api/exe-swap/current" => Self::ExeCurrent,
            "/api/exe-swap/enhanced" => Self::ExeEnhanced,
            "/api/update-check" => Self::UpdateCheck,
            "/api/reset-stock" => Self::ResetStock,
            "/api/mod-files/clear" => Self::ClearModFiles,
            _ => return None,
        })
    }

    /// The frontend `busy` id used by PR #40.
    #[allow(dead_code)]
    pub fn id(self) -> &'static str {
        match self {
            Self::T7Install => "t7-install",
            Self::T7Uninstall => "t7-uninstall",
            Self::DxvkInstall => "dxvk-install",
            Self::DxvkUninstall => "dxvk-uninstall",
            Self::EnhancedValidate => "enhanced-validate",
            Self::EnhancedInstall => "enhanced-install",
            Self::EnhancedUninstall => "enhanced-uninstall",
            Self::WorkshopInstall => "workshop-install",
            Self::ExeCompatible => "exe-compatible",
            Self::ExeCurrent => "exe-current",
            Self::ExeEnhanced => "exe-enhanced",
            Self::UpdateCheck => "update-check",
            Self::ResetStock => "reset-stock",
            Self::ClearModFiles => "clear-mod-files",
        }
    }

    /// Inverse of [`Operation::id`], for events that carry the `busy` id.
    #[allow(dead_code)]
    pub fn from_id(id: &str) -> Option<Self> {
        const ALL: [Operation; 14] = [
            Operation::T7Install,
            Operation::T7Uninstall,
            Operation::DxvkInstall,
            Operation::DxvkUninstall,
            Operation::EnhancedValidate,
            Operation::EnhancedInstall,
            Operation::EnhancedUninstall,
            Operation::WorkshopInstall,
            Operation::ExeCompatible,
            Operation::ExeCurrent,
            Operation::ExeEnhanced,
            Operation::UpdateCheck,
            Operation::ResetStock,
            Operation::ClearModFiles,
        ];
        ALL.into_iter().find(|op| op.id() == id)
    }

    fn title(self) -> &'static str {
        match self {
            Self::T7Install => "Installing T7 Patch",
            Self::T7Uninstall => "Uninstalling T7 Patch",
            Self::DxvkInstall => "Installing DXVK-GPLAsync",
            Self::DxvkUninstall => "Uninstalling DXVK-GPLAsync",
            Self::EnhancedValidate => "Validating dump source",
            Self::EnhancedInstall => "Installing BO3 Enhanced",
            Self::EnhancedUninstall => "Uninstalling BO3 Enhanced",
            Self::WorkshopInstall => "Installing workshop mod",
            Self::ExeCompatible => "Switching to the Compatible build",
            Self::ExeCurrent => "Switching to the Latest build",
            Self::ExeEnhanced => "Switching to BO3 Enhanced",
            Self::UpdateCheck => "Checking for updates",
            Self::ResetStock => "Resetting to stock",
            Self::ClearModFiles => "Clearing mod files",
        }
    }

    /// The stage a streamed log line implies, matching the messages PR #40's
    /// core logs ("Downloading DXVK-GPLAsync...", "Fetching latest BO3
    /// Enhanced release...", "Installing BO3 Enhanced files...").
    fn stage_for_log(message: &str) -> Option<&'static str> {
        let lower = message.to_lowercase();
        [
            ("download", "Downloading"),
            ("fetching", "Fetching release"),
            ("extract", "Extracting"),
            ("verif", "Verifying"),
            ("validat", "Validating"),
            ("back", "Backing up"),
            ("install", "Installing"),
            ("remov", "Removing files"),
            ("restor", "Restoring"),
        ]
        .into_iter()
        .find(|(needle, _)| lower.contains(needle))
        .map(|(_, stage)| stage)
    }
}

/// What the UI shows for the operation in flight (or just finished).
pub struct OperationProgress {
    pub op: Operation,
    /// Current stage, e.g. "Downloading". Empty until the first stage is known.
    pub stage: String,
    /// `Some(0.0..=1.0)` draws a determinate bar, `None` an indeterminate one.
    pub fraction: Option<f32>,
    /// Latest log line while running; the outcome text once finished.
    pub message: String,
    /// `None` while running, `Some(success)` once finished.
    pub outcome: Option<bool>,
    started: Instant,
    finished: Option<Instant>,
}

impl OperationProgress {
    fn new(op: Operation) -> Self {
        Self {
            op,
            stage: String::new(),
            fraction: None,
            message: String::new(),
            outcome: None,
            started: Instant::now(),
            finished: None,
        }
    }

    fn running(&self) -> bool {
        self.outcome.is_none()
    }

    /// The one-line summary used by the strip and the Activity Log header.
    pub fn headline(&self) -> String {
        match self.outcome {
            Some(true) => "Done".to_owned(),
            Some(false) => "Failed".to_owned(),
            None if self.stage.is_empty() => format!("{}…", self.op.title()),
            None => format!("{} · {}…", self.op.title(), self.stage),
        }
    }
}

/// An event from the backend (or the UI itself) about a running operation.
#[derive(Clone, Debug)]
pub enum ProgressEvent {
    /// An operation began. The UI raises this itself from `send`.
    Started(Operation),
    /// A new stage and, when the core can measure it, how far along it is.
    #[allow(dead_code)]
    Stage {
        op: Operation,
        stage: String,
        fraction: Option<f32>,
    },
    /// One activity-log line, i.e. PR #40's `patchops-log` payload.
    Log {
        category: String,
        message: String,
        /// The timestamped line (`"<time> - <Category>: <message>"`). Give it
        /// when available: it is what de-duplicates a streamed entry against
        /// the same entry arriving in the next status snapshot.
        line: Option<String>,
    },
    /// The operation ended. The UI raises this itself from `handle_reply`.
    Finished {
        op: Operation,
        ok: bool,
        message: String,
    },
}

impl ProgressEvent {
    /// Shorthand for the `patchops-log` mapping.
    #[allow(dead_code)]
    pub fn log(
        category: impl Into<String>,
        message: impl Into<String>,
        line: Option<String>,
    ) -> Self {
        Self::Log {
            category: category.into(),
            message: message.into(),
            line,
        }
    }
}

/// `.progress` track: a determinate fill, or a sliding chunk when the length
/// of the work is unknown.
pub fn progress_bar(fraction: Option<f32>, tone: Hsla) -> Div {
    let track = div()
        .relative()
        .flex_none()
        .h(px(4.))
        .w_full()
        .overflow_hidden()
        .rounded_full()
        .bg(theme::white(0.1));
    match fraction {
        Some(fraction) => track.child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0., 1.)))
                .rounded_full()
                .bg(tone),
        ),
        None => track.child(
            div()
                .absolute()
                .top_0()
                .h_full()
                .w(relative(0.3))
                .rounded_full()
                .bg(tone)
                .with_animation(
                    "progress-indeterminate",
                    Animation::new(Duration::from_millis(1300)).repeat(),
                    |chunk, delta| chunk.left(relative(delta * 1.3 - 0.3)),
                ),
        ),
    }
}

impl ControlCenter {
    /// Single entry point for operation feedback.
    ///
    /// Call this from the backend's event hook, on the UI thread, for each
    /// event. The existing 100 ms drain loop in `ControlCenter::new` is the
    /// natural spot:
    ///
    /// ```ignore
    /// while let Ok(event) = view.backend.events.try_recv() {
    ///     match event {
    ///         // `patchops-log` in PR #40, `LogEntry { category, message, line }`.
    ///         BackendEvent::Log(entry) => view.on_progress(
    ///             ProgressEvent::log(entry.category, entry.message, Some(entry.line)),
    ///             cx,
    ///         ),
    ///         // Optional: only when the core can report a stage or a fraction.
    ///         // BackendEvent::Stage { op, stage, fraction } => view.on_progress(
    ///         //     ProgressEvent::Stage { op, stage, fraction }, cx),
    ///     }
    /// }
    /// ```
    ///
    /// Event mapping from PR #40:
    /// - `patchops-log {category, message, line}` -> [`ProgressEvent::Log`].
    ///   The line streams into the Activity Log at once and, while an
    ///   operation runs, becomes the strip's message (and its stage, when the
    ///   text names one: "Downloading ...", "Installing ...").
    /// - the frontend's `busy` id (`t7-install`, `dxvk-install`,
    ///   `enhanced-install`, `workshop-install`, ...) -> [`Operation`]. The UI
    ///   already raises [`ProgressEvent::Started`] and
    ///   [`ProgressEvent::Finished`] itself from the request it sent, so the
    ///   hook never has to; use [`Operation::from_id`] if a core event names
    ///   an operation by that id.
    /// - there is no byte/percent event upstream. [`ProgressEvent::Stage`]
    ///   with `fraction: Some(..)` is the optional hook for one; without it
    ///   the strip stays indeterminate.
    pub fn on_progress(&mut self, event: ProgressEvent, cx: &mut Context<Self>) {
        match event {
            ProgressEvent::Started(op) => {
                self.progress = Some(OperationProgress::new(op));
            }
            ProgressEvent::Stage {
                op,
                stage,
                fraction,
            } => {
                let progress = self.progress_for(op);
                progress.stage = stage;
                progress.fraction = fraction.map(|value| value.clamp(0., 1.));
            }
            ProgressEvent::Log {
                category,
                message,
                line,
            } => {
                if let Some(progress) = self.progress.as_mut().filter(|p| p.running()) {
                    if let Some(stage) = Operation::stage_for_log(&message) {
                        progress.stage = stage.to_owned();
                    }
                    progress.message = message.clone();
                }
                self.live_logs.push(json!({
                    "category": category,
                    "message": message,
                    "line": line,
                }));
                if self.live_logs.len() > LIVE_LOG_LIMIT {
                    self.live_logs
                        .drain(..self.live_logs.len() - LIVE_LOG_LIMIT);
                }
                self.follow_log();
            }
            ProgressEvent::Finished { op, ok, message } => {
                let progress = self.progress_for(op);
                progress.outcome = Some(ok);
                progress.finished = Some(Instant::now());
                progress.fraction = Some(1.);
                progress.message = message;
            }
        }
        cx.notify();
    }

    /// The tracked operation, started on demand if `op` is not the current one.
    fn progress_for(&mut self, op: Operation) -> &mut OperationProgress {
        if self
            .progress
            .as_ref()
            .is_none_or(|progress| progress.op != op)
        {
            self.progress = Some(OperationProgress::new(op));
        }
        self.progress.as_mut().expect("progress was just set")
    }

    /// Drop a finished operation once it has been on screen long enough.
    pub(super) fn expire_progress(&mut self, cx: &mut Context<Self>) {
        let expired = self.progress.as_ref().is_some_and(|progress| {
            progress
                .finished
                .is_some_and(|at| at.elapsed() >= OUTCOME_LINGER)
        });
        if expired {
            self.progress = None;
            cx.notify();
        }
    }

    /// The Activity Log header text: the running stage, else the last status.
    pub(super) fn status_line(&self) -> String {
        match &self.progress {
            Some(progress) if progress.running() => progress.headline(),
            _ => self.message.clone(),
        }
    }

    /// The strip for the operation in flight, if it is one of `ops`.
    pub(super) fn progress_strip(&self, ops: &[Operation]) -> Option<Div> {
        let progress = self.progress.as_ref().filter(|p| ops.contains(&p.op))?;
        let (tone, glyph) = match progress.outcome {
            None => (theme::accent(), None),
            Some(true) => (theme::ok(), Some(Glyph::CheckCircle)),
            Some(false) => (theme::danger(), Some(Glyph::Alert)),
        };
        let detail = match (progress.outcome, progress.message.as_str()) {
            (None, "") => String::new(),
            (_, message) => message.lines().next().unwrap_or_default().to_owned(),
        };
        let trailing = match (progress.outcome, progress.fraction) {
            (None, Some(fraction)) => format!("{:.0}%", fraction * 100.),
            (None, None) => format!("{}s", progress.started.elapsed().as_secs()),
            _ => String::new(),
        };
        let title = match progress.outcome {
            None => progress.headline(),
            Some(_) => format!("{}: {}", progress.op.title(), progress.headline()),
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .w_full()
                .min_w_0()
                .p(px(10.))
                .border_1()
                .border_color(if progress.outcome.is_some() {
                    tone.alpha(0.34)
                } else {
                    theme::border()
                })
                .rounded(theme::radius_card())
                .bg(theme::white(0.03))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(theme::FONT_SM))
                        .font_weight(FontWeight::BOLD)
                        .when_some(glyph, |row, glyph| row.child(icon(glyph, 15., tone)))
                        .child(div().flex_1().min_w_0().truncate().child(title))
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(theme::FONT_XS))
                                .text_color(theme::muted())
                                .child(trailing),
                        ),
                )
                .child(progress_bar(progress.fraction, tone))
                .when(!detail.is_empty(), |strip| {
                    // flex_1 + min_w_0 gives the text a definite width; a bare
                    // `truncate` child measures at zero width and shows "…".
                    strip.child(
                        div().flex().child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(theme::FONT_XS))
                                .text_color(theme::muted())
                                .child(detail),
                        ),
                    )
                }),
        )
    }
}
