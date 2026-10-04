//! The PatchOpsIII desktop UI. The layout mirrors the Electron renderer
//! (`src/renderer/main.tsx` + `styles/app.css`); each page lives in its own module.
mod assets;
mod chrome;
mod components;
mod dashboard;
mod enhanced;
mod exe;
mod graphics;
mod modals;
mod progress;
mod t7;
mod theme;
mod tools;

pub use assets::Assets;
pub use chrome::{log_session, window_options};
// The backend's event hook builds these; see `ControlCenter::on_progress`.
pub use progress::{Operation, ProgressEvent};

use crate::backend::{Backend, BackendEvent, Reply, Request};
use components::{Btn, Glyph, icon};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    slider::{SliderEvent, SliderState},
};
use progress::OperationProgress;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// How often the UI asks the backend for a fresh status document while idle.
/// Actions refresh explicitly (see `request_refresh`), so this is only the
/// safety net for changes made outside the app; raise it freely.
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// How often the Steam depot prompt re-checks whether the depot has arrived.
const DEPOT_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// How often the UI loop drains backend replies and events.
const UI_TICK: Duration = Duration::from_millis(100);
const DEPOT_COMMAND_MARKER: &str = "Steam console command: ";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Dashboard,
    T7,
    Exe,
    Enhanced,
    Graphics,
    Tools,
}

impl Page {
    const ALL: [Page; 6] = [
        Page::Dashboard,
        Page::T7,
        Page::Exe,
        Page::Enhanced,
        Page::Graphics,
        Page::Tools,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Dashboard => "Dashboard",
            Page::T7 => "T7 Patch",
            Page::Exe => "EXE Swapper",
            Page::Enhanced => "Enhanced",
            Page::Graphics => "Graphics",
            Page::Tools => "Tools",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Page::Dashboard => Glyph::Dashboard,
            Page::T7 => Glyph::Shield,
            Page::Exe => Glyph::Refresh,
            Page::Enhanced => Glyph::Gem,
            Page::Graphics => Glyph::Image,
            Page::Tools => Glyph::Wrench,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GraphicsTab {
    Settings,
    Dxvk,
}

/// Result of the last "Validate Source" run (Enhanced page).
struct Validation {
    label: String,
    ok: Option<bool>,
    checked_at: Option<String>,
    source: String,
}

impl Validation {
    fn not_run() -> Self {
        Self {
            label: "Not run".into(),
            ok: None,
            checked_at: None,
            source: String::new(),
        }
    }
}

/// The Steam console prompt shown when the compatible depot is missing.
struct Depot {
    command: String,
    copied: bool,
    watching: bool,
    last_poll: Instant,
}

type Staged = (String, Vec<Request>);

pub struct ControlCenter {
    backend: Backend,
    state: Value,
    page: Page,
    graphics_tab: GraphicsTab,
    busy: bool,
    connected: bool,
    message: String,
    failed: bool,
    last_refresh: Instant,
    /// Path of the request currently owned by the backend worker.
    inflight: String,
    inputs: BTreeMap<&'static str, Entity<InputState>>,
    sliders: BTreeMap<&'static str, Entity<SliderState>>,
    /// An action waiting for the user's confirmation. Multi-step actions
    /// (such as "Apply All") stage several requests that run in order.
    pending: Option<Staged>,
    queue: VecDeque<Request>,
    selected_profile: String,
    seen_profile: String,
    t7_color: String,
    t7_password_enabled: bool,
    t7_password_touched: bool,
    show_current_password: bool,
    show_new_password: bool,
    validation: Validation,
    depot: Option<Depot>,
    dxvk_draft: Value,
    dxvk_synced: Value,
    advanced_open: bool,
    danger_open: bool,
    content_scroll: ScrollHandle,
    log_scroll: ScrollHandle,
    log_count: usize,
    /// The long operation in flight (or just finished); see `progress.rs`.
    progress: Option<OperationProgress>,
    /// The operation behind `inflight`, when it gets a progress strip.
    inflight_op: Option<Operation>,
    /// Log lines streamed through `on_progress` since the last snapshot.
    live_logs: Vec<Value>,
    /// A status refresh is wanted as soon as the backend is idle.
    refresh_pending: bool,
    /// Titlebar drag in progress; see `chrome::drag_region`.
    title_drag: chrome::DragArmed,
    /// Whether the first presented frame has been reported; see `render`.
    first_frame: bool,
}

fn dxvk_recommended() -> Value {
    json!({
        "enableAsync": true,
        "gplAsyncCache": true,
        "numCompilerThreads": 0,
        "maxFrameRate": 0,
        "maxFrameLatency": 1,
        "tearFree": "True",
        "hudEnabled": false,
    })
}

/// `formatTimestamp` from the Electron renderer, without a locale database.
fn format_timestamp(value: &str) -> String {
    if value.is_empty() {
        return "Never".into();
    }
    value
        .split(['.', '+', 'Z'])
        .next()
        .unwrap_or(value)
        .replace('T', " ")
}

fn clock_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let day = secs % 86_400;
    format!(
        "{:02}:{:02}:{:02} UTC",
        day / 3600,
        day % 3600 / 60,
        day % 60
    )
}

impl ControlCenter {
    pub fn new(backend: Backend, window: &mut Window, cx: &mut Context<Self>) -> Self {
        theme::apply(cx);
        let inputs: BTreeMap<_, _> = [
            ("directory", "Black Ops III directory"),
            ("gamertag", "Gamertag (up to 20 characters)"),
            ("password", "Enter network password"),
            ("dump", "Path to DUMP.zip or an extracted folder"),
            ("fps", "0–1000"),
            ("resolution", "1920x1080"),
            ("refresh", "1–1000"),
            ("latency", "0–4"),
            ("vram", "75–100"),
            ("dxvk-threads", "0–64"),
            ("dxvk-latency", "0–16"),
        ]
        .into_iter()
        .map(|(key, placeholder)| {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .masked(key == "password")
            });
            (key, input)
        })
        .collect();
        for input in inputs.values() {
            // Drafts feed buttons and badges, so repaint while the user types.
            cx.subscribe_in(
                input,
                window,
                |_, _, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => cx.notify(),
                    InputEvent::PressEnter { .. } => {}
                    InputEvent::Focus | InputEvent::Blur => {}
                },
            )
            .detach();
        }
        cx.subscribe_in(
            &inputs["directory"],
            window,
            |view, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    view.stage_directory(cx);
                }
            },
        )
        .detach();
        let sliders: BTreeMap<_, _> = [
            ("render", 50., 200., 100.),
            ("fov", 65., 120., 80.),
            ("dxvk-fps", 0., 360., 0.),
        ]
        .into_iter()
        .map(|(key, min, max, value)| {
            let slider = cx.new(|_| {
                SliderState::new()
                    .min(min)
                    .max(max)
                    .step(1.)
                    .default_value(value)
            });
            (key, slider)
        })
        .collect();
        for slider in sliders.values() {
            cx.subscribe_in(slider, window, |_, _, _: &SliderEvent, _, cx| cx.notify())
                .detach();
        }
        // Developer knob for screenshots: PATCHOPSIII_GPUI_PAGE=t7|exe|enhanced|graphics|dxvk|tools.
        let (page, graphics_tab) = match std::env::var("PATCHOPSIII_GPUI_PAGE").as_deref() {
            Ok("t7") => (Page::T7, GraphicsTab::Settings),
            Ok("exe") => (Page::Exe, GraphicsTab::Settings),
            Ok("enhanced") => (Page::Enhanced, GraphicsTab::Settings),
            Ok("graphics") => (Page::Graphics, GraphicsTab::Settings),
            Ok("dxvk") => (Page::Graphics, GraphicsTab::Dxvk),
            Ok("tools") => (Page::Tools, GraphicsTab::Settings),
            _ => (Page::Dashboard, GraphicsTab::Settings),
        };
        let mut view = Self {
            backend,
            state: Value::Null,
            page,
            graphics_tab,
            busy: false,
            connected: false,
            message: "Connecting to the local service…".into(),
            failed: false,
            last_refresh: Instant::now(),
            inflight: String::new(),
            inputs,
            sliders,
            pending: None,
            queue: VecDeque::new(),
            selected_profile: "default".into(),
            seen_profile: String::new(),
            t7_color: String::new(),
            t7_password_enabled: false,
            t7_password_touched: false,
            show_current_password: false,
            show_new_password: false,
            validation: Validation::not_run(),
            depot: None,
            dxvk_draft: dxvk_recommended(),
            dxvk_synced: Value::Null,
            advanced_open: false,
            danger_open: false,
            content_scroll: ScrollHandle::new(),
            log_scroll: ScrollHandle::new(),
            log_count: 0,
            progress: None,
            inflight_op: None,
            live_logs: Vec::new(),
            refresh_pending: false,
            title_drag: Default::default(),
            first_frame: false,
        };
        // Pick the window background for the decorations the platform granted, and redo
        // it if they change later; repaint when the frame (tiling, maximise)
        // changes shape.
        // GPUI only forwards `TitlebarOptions::title` on X11/Windows/macOS.
        window.set_window_title("PatchOpsIII");
        chrome::settle(window);
        cx.observe_window_appearance(window, |_, window, cx| {
            chrome::settle(window);
            cx.notify();
        })
        .detach();
        cx.observe_window_bounds(window, |_, _, cx| cx.notify())
            .detach();
        view.send("/api/status", None, cx);
        // Drain replies and periodically refresh on GPUI's foreground executor.
        // The only blocking work lives on the dedicated HTTP worker.
        cx.spawn_in(window, async move |entity, cx| {
            loop {
                cx.background_executor().timer(UI_TICK).await;
                if entity
                    .update_in(cx, |view, window, cx| {
                        // Events precede their operation's reply on the worker.
                        while let Ok(event) = view.backend.events.try_recv() {
                            view.handle_backend_event(event, cx);
                        }
                        while let Ok(reply) = view.backend.replies.try_recv() {
                            view.handle_reply(reply, window, cx);
                        }
                        view.poll(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        view
    }

    /// Maps worker events onto `on_progress`. Started/finished are raised by
    /// the UI from the request and its reply, so only intermediate stages pass.
    fn handle_backend_event(&mut self, event: BackendEvent, cx: &mut Context<Self>) {
        match event {
            BackendEvent::Log(entry) => self.on_progress(
                ProgressEvent::log(entry.category, entry.message, Some(entry.line)),
                cx,
            ),
            BackendEvent::Progress(progress) => {
                if matches!(progress.stage.as_str(), "started" | "completed" | "failed") {
                    return;
                }
                if let Some(op) = Operation::from_path(&progress.op) {
                    self.on_progress(
                        ProgressEvent::Stage {
                            op,
                            stage: progress.stage,
                            fraction: progress.fraction,
                        },
                        cx,
                    );
                }
            }
        }
    }

    fn handle_reply(&mut self, reply: Reply, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = false;
        let path = std::mem::take(&mut self.inflight);
        let (is_status, carried_state) = (reply.is_status, reply.state.is_some());
        // A depot prompt is a question for the user, not a failed operation.
        let failed = reply.failed && !reply.message.contains(DEPOT_COMMAND_MARKER);
        if let Some(op) = self.inflight_op.take() {
            let message = reply.message.lines().next().unwrap_or_default().to_owned();
            self.on_progress(
                ProgressEvent::Finished {
                    op,
                    ok: !failed,
                    message,
                },
                cx,
            );
        }
        if reply.failed {
            self.queue.clear();
            self.failed = true;
            if reply.is_status {
                self.connected = false;
            }
            self.on_failure(&path, &reply.message);
            self.message = reply.message;
        } else {
            let initial = self.state.is_null();
            let reconnected = !self.connected;
            if let Some(state) = reply.state {
                self.state = state;
                self.connected = true;
                if initial {
                    self.load_inputs(window, cx);
                } else {
                    self.sync_drafts(window, cx);
                }
            }
            self.on_success(&path, &reply.message, window, cx);
            if !reply.is_status || reconnected || self.message.starts_with("Connecting") {
                self.failed = false;
                self.message = reply.message;
            }
        }
        self.last_refresh = Instant::now();
        if carried_state {
            self.prune_live_logs();
        }
        self.refresh_after(is_status, failed, carried_state);
        self.follow_log();
        if let Some(next) = self.queue.pop_front() {
            self.send(&next.path, next.body, cx);
        }
        cx.notify();
    }

    /// Keep the Activity Log pinned to its newest line when entries arrive.
    fn follow_log(&mut self) {
        let entries = self.visible_logs().len();
        if entries != self.log_count {
            self.log_count = entries;
            self.log_scroll.scroll_to_bottom();
        }
    }

    /// Refresh-after-action policy, in one place. A successful action normally
    /// answers with the new state document, so nothing more is needed. One that
    /// answers without it, or fails (the backend may have changed state and
    /// logged before failing), asks for a status refresh right away instead of
    /// waiting out `STATUS_POLL_INTERVAL`.
    fn refresh_after(&mut self, is_status: bool, failed: bool, carried_state: bool) {
        if !is_status && (failed || !carried_state) {
            self.request_refresh();
        }
    }

    /// Ask for a status refresh as soon as the backend is idle. Backend events
    /// that mean "state changed outside a request" can call this too.
    pub fn request_refresh(&mut self) {
        self.refresh_pending = true;
    }

    /// Per-endpoint bookkeeping for a failed request.
    fn on_failure(&mut self, path: &str, message: &str) {
        match path {
            "/api/enhanced-validate" => {
                self.validation = Validation {
                    label: message.to_owned(),
                    ok: Some(false),
                    checked_at: Some(clock_now()),
                    source: self.validation.source.clone(),
                };
            }
            "/api/exe-swap/compatible" => {
                if let Some((_, command)) = message.split_once(DEPOT_COMMAND_MARKER) {
                    // The depot is not downloaded yet: this is a prompt, not an error.
                    let watching = self.depot.as_ref().is_some_and(|depot| depot.watching);
                    let copied = self.depot.as_ref().is_some_and(|depot| depot.copied);
                    self.depot = Some(Depot {
                        command: command.trim().to_owned(),
                        copied,
                        watching,
                        last_poll: Instant::now(),
                    });
                    self.failed = false;
                } else if let Some(depot) = self.depot.as_mut() {
                    depot.watching = false;
                }
            }
            _ => {}
        }
    }

    /// Per-endpoint bookkeeping for a successful request.
    fn on_success(
        &mut self,
        path: &str,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match path {
            "/api/enhanced-validate" => {
                self.validation = Validation {
                    label: message.to_owned(),
                    ok: Some(true),
                    checked_at: Some(clock_now()),
                    source: self.text("dump", cx).trim().to_owned(),
                };
            }
            "/api/exe-swap/compatible" => self.depot = None,
            "/api/logs/clear" => self.live_logs.clear(),
            "/api/t7-config" => {
                self.t7_password_touched = false;
                self.load_t7(window, cx);
            }
            _ => {}
        }
    }

    /// Background work: status refresh and depot watching.
    fn poll(&mut self, cx: &mut Context<Self>) {
        self.expire_progress(cx);
        if self.busy || self.pending.is_some() {
            return;
        }
        let watching = self.depot.as_ref().is_some_and(|depot| {
            depot.watching && depot.last_poll.elapsed() >= DEPOT_POLL_INTERVAL
        });
        if watching {
            if let Some(depot) = self.depot.as_mut() {
                depot.last_poll = Instant::now();
            }
            self.send("/api/exe-swap/compatible", Some(json!({})), cx);
        } else if self.refresh_pending || self.last_refresh.elapsed() >= STATUS_POLL_INTERVAL {
            self.refresh_pending = false;
            self.send("/api/status", None, cx);
        }
    }

    fn send(&mut self, path: &str, body: Option<Value>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match self.backend.requests.send(Request {
            path: path.into(),
            body,
        }) {
            Ok(()) => {
                self.busy = true;
                self.inflight = path.to_owned();
                self.last_refresh = Instant::now();
                // The depot watcher re-sends the same request every few
                // seconds; that is polling, not an operation to announce.
                let watching = self.depot.as_ref().is_some_and(|depot| depot.watching);
                self.inflight_op = Operation::from_path(path)
                    .filter(|op| !(watching && *op == Operation::ExeCompatible));
                if let Some(op) = self.inflight_op {
                    self.on_progress(ProgressEvent::Started(op), cx);
                }
            }
            Err(_) => {
                self.failed = true;
                self.connected = false;
                self.message = "Local service worker stopped. Restart PatchOpsIII.".into();
            }
        }
        cx.notify();
    }

    fn stage(&mut self, label: String, path: String, body: Value, cx: &mut Context<Self>) {
        self.stage_all(
            label,
            vec![Request {
                path,
                body: Some(body),
            }],
            cx,
        );
    }

    fn stage_all(&mut self, label: String, requests: Vec<Request>, cx: &mut Context<Self>) {
        self.pending = Some((label, requests));
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if let Some((label, mut requests)) = self.pending.take() {
            self.message = format!("Running: {label}…");
            self.failed = false;
            if requests.is_empty() {
                cx.notify();
                return;
            }
            let first = requests.remove(0);
            self.queue = requests.into();
            self.send(&first.path, first.body, cx);
        }
    }

    fn reject(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.message = message.into();
        self.failed = true;
        cx.notify();
    }

    fn text(&self, key: &str, cx: &App) -> String {
        self.inputs[key].read(cx).value().to_string()
    }

    fn set_text(&self, key: &str, value: String, window: &mut Window, cx: &mut Context<Self>) {
        self.inputs[key].update(cx, |state, cx| state.set_value(value, window, cx));
    }

    fn set_slider(&self, key: &str, value: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.sliders[key].update(cx, |state, cx| state.set_value(value as f32, window, cx));
    }

    fn slider_value(&self, key: &str, cx: &App) -> i64 {
        self.sliders[key].read(cx).value().start().round() as i64
    }

    /// Copy every draft control from the backend state (initial load and
    /// "Reload Values").
    fn load_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (key, pointer) in [
            ("directory", "/gameDir"),
            ("dump", "/enhanced/dumpSource"),
            ("fps", "/graphics/maxFps"),
            ("resolution", "/graphics/resolution"),
            ("refresh", "/graphics/refreshRate"),
            ("latency", "/advanced/maxFrameLatency"),
            ("vram", "/advanced/vramTarget"),
        ] {
            let value = self.string(pointer);
            self.set_text(key, value, window, cx);
        }
        for (key, pointer) in [
            ("render", "/graphics/renderResolution"),
            ("fov", "/graphics/fov"),
        ] {
            let value = self.int(pointer, 0);
            self.set_slider(key, value, window, cx);
        }
        self.load_t7(window, cx);
        self.t7_password_touched = false;
        self.seen_profile.clear();
        self.dxvk_synced = Value::Null;
        self.sync_drafts(window, cx);
    }

    fn load_t7(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.string("/t7/plainName");
        self.set_text("gamertag", name, window, cx);
        self.t7_color = self.string("/t7/colorCode");
    }

    /// Drafts that follow the backend unless the user is editing them.
    fn sync_drafts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.string("/activeLaunchProfile");
        if active != self.seen_profile {
            self.selected_profile = if active.is_empty() || active == "custom" {
                "default".into()
            } else {
                active.clone()
            };
            self.seen_profile = active;
        }
        if !self.t7_password_touched {
            self.t7_password_enabled = !self.string("/t7/networkPassword").is_empty();
        }
        let settings = self
            .state
            .pointer("/dxvk/settings")
            .cloned()
            .unwrap_or(Value::Null);
        if settings != self.dxvk_synced && settings.is_object() {
            self.dxvk_draft = settings.clone();
            self.dxvk_synced = settings;
            self.show_dxvk_draft(window, cx);
        }
    }

    fn show_dxvk_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let threads = self.dxvk_int("numCompilerThreads").to_string();
        let latency = self.dxvk_int("maxFrameLatency").to_string();
        let fps = self.dxvk_int("maxFrameRate");
        self.set_text("dxvk-threads", threads, window, cx);
        self.set_text("dxvk-latency", latency, window, cx);
        self.set_slider("dxvk-fps", fps, window, cx);
    }

    fn flag(&self, pointer: &str) -> bool {
        self.state
            .pointer(pointer)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// A state value as display text; absent and null values become "".
    fn string(&self, pointer: &str) -> String {
        match self.state.pointer(pointer) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n
                .as_f64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| n.to_string()),
            Some(Value::Null) | None => String::new(),
            Some(v) => v.to_string(),
        }
    }

    /// A state value as display text with a visible fallback.
    fn value(&self, pointer: &str) -> String {
        let value = self.string(pointer);
        if value.is_empty() {
            "—".into()
        } else {
            value
        }
    }

    fn int(&self, pointer: &str, default: i64) -> i64 {
        self.state
            .pointer(pointer)
            .and_then(Value::as_f64)
            .map_or(default, |n| n.round() as i64)
    }

    fn dxvk_flag(&self, key: &str) -> bool {
        self.dxvk_draft[key].as_bool().unwrap_or(false)
    }

    fn dxvk_int(&self, key: &str) -> i64 {
        self.dxvk_draft[key].as_i64().unwrap_or(0)
    }

    fn can_act(&self) -> bool {
        !self.busy && self.connected && self.pending.is_none()
    }

    /// A click handler that asks for confirmation before sending `path`.
    fn stage_click(
        &self,
        cx: &mut Context<Self>,
        label: impl Into<String>,
        path: &str,
        body: Value,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let label = label.into();
        let path = path.to_owned();
        cx.listener(move |view, _: &ClickEvent, _, cx| {
            view.stage(label.clone(), path.clone(), body.clone(), cx)
        })
    }

    /// A button that stages one request for confirmation.
    fn action(
        &self,
        cx: &mut Context<Self>,
        id: impl Into<ElementId>,
        label: &str,
        path: &str,
        body: Value,
    ) -> Btn {
        Btn::new(id, label.to_owned())
            .disabled(!self.can_act())
            .on_click(self.stage_click(cx, label, path, body))
    }

    fn config_body(key: &str, value: Value) -> Value {
        let comment = match key {
            "MaxFPS" => "Maximum FPS cap",
            "FOV" => "Field of view",
            "FullScreenMode" => "0=Windowed,1=Fullscreen,2=Fullscreen Windowed",
            "WindowSize" => "any text",
            "RefreshRate" => "1 to 240",
            "ResolutionPercent" => "50 to 200",
            "Vsync" => "Vertical sync",
            "DrawFPS" => "FPS counter",
            "SmoothFramerate" => "Frame smoothing",
            "RestrictGraphicsOptions" => "Expose all graphics options",
            "SerializeRender" => "Reduce CPU pressure",
            "MaxFrameLatency" => "Maximum frame latency",
            _ => "Managed by PatchOpsIII",
        };
        json!({"key": key, "value": value, "comment": comment})
    }

    /// A text input styled like the Electron `input` elements.
    fn field(&self, key: &str) -> Input {
        Input::new(&self.inputs[key])
            .disabled(self.busy)
            .min_h(px(theme::CONTROL_HEIGHT - 2.))
            .bg(theme::field())
            .rounded(theme::radius_card())
            .border_color(theme::border())
    }

    fn stage_directory(&mut self, cx: &mut Context<Self>) {
        let path = self.text("directory", cx);
        let path = path.trim().to_owned();
        if path.is_empty() {
            self.reject("Select or enter a directory first.", cx);
        } else if path != self.string("/gameDir") {
            self.stage(
                format!("Use directory {path}"),
                "/api/game-directory".into(),
                json!({"path": path}),
                cx,
            );
        }
    }

    fn choose_directory(&mut self, input: &'static str, window: &Window, cx: &mut Context<Self>) {
        self.choose_path(input, false, window, cx);
    }

    /// Native picker for a folder or, with `file`, a single file. Platform
    /// pickers can't offer both at once, so callers expose one per action.
    fn choose_path(
        &mut self,
        input: &'static str,
        file: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: file,
            directories: !file,
            multiple: false,
            prompt: Some(
                if file {
                    "Select a file"
                } else {
                    "Select a folder"
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |entity, cx| match paths.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.first() {
                    let value = path.to_string_lossy().into_owned();
                    let _ = entity.update_in(cx, |view, window, cx| {
                        view.set_text(input, value, window, cx);
                        view.after_pick(input, cx);
                    });
                }
            }
            Ok(Ok(None)) => {}
            _ => {
                let _ = entity.update(cx, |view, cx| {
                    view.reject("File picker unavailable. Enter the path directly.", cx);
                });
            }
        })
        .detach();
    }

    /// What picking a folder means for each input.
    fn after_pick(&mut self, input: &str, cx: &mut Context<Self>) {
        match input {
            "directory" => self.stage_directory(cx),
            "dump" => {
                self.validation = Validation::not_run();
                cx.notify();
            }
            _ => {}
        }
    }

    /// Latest log lines, oldest first, without the service start-up notice.
    fn log_key(entry: &Value) -> String {
        format!(
            "{}|{}|{}",
            entry["line"], entry["category"], entry["message"]
        )
    }

    /// Streamed lines the status snapshot now contains no longer need keeping.
    fn prune_live_logs(&mut self) {
        let known: std::collections::HashSet<String> = self.state["logs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(Self::log_key)
            .collect();
        self.live_logs
            .retain(|entry| !known.contains(&Self::log_key(entry)));
    }

    /// Snapshot entries followed by lines streamed since (`on_progress`).
    fn visible_logs(&self) -> Vec<(String, String)> {
        let mut seen = std::collections::HashSet::new();
        let mut entries: Vec<(String, String)> = self.state["logs"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(&self.live_logs)
            .filter(|entry| entry["message"] != "PatchOpsIII local API started.")
            .filter(|entry| seen.insert(Self::log_key(entry)))
            .map(|entry| {
                (
                    entry["category"].as_str().unwrap_or("Info").to_owned(),
                    entry["message"].as_str().unwrap_or("").to_owned(),
                )
            })
            .collect();
        if entries.len() > 120 {
            entries.drain(..entries.len() - 120);
        }
        entries
    }

    fn page_view(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.page {
            Page::Dashboard => self.dashboard_view(cx),
            Page::T7 => self.t7_view(cx),
            Page::Exe => self.exe_view(cx),
            Page::Enhanced => self.enhanced_view(cx),
            Page::Graphics => self.graphics_view(cx),
            Page::Tools => self.tools_view(cx),
        }
    }

    /// `.titlebar`: logo, name, version (click to check for updates), the
    /// connection state and the caption buttons. The whole strip is the
    /// window's titlebar; everything except the buttons drags the window.
    fn header(
        &self,
        frame: chrome::Frame,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let version = self.string("/appVersion");
        let version = if version.to_lowercase().starts_with('v') || version.is_empty() {
            version
        } else {
            format!("v{version}")
        };
        let can_act = self.can_act();
        let drag = &self.title_drag;
        chrome::titlebar(frame)
            .child(
                chrome::drag_region("titlebar-brand", drag, frame)
                    .flex()
                    .items_center()
                    .flex_none()
                    .h_full()
                    .pl(px(10.))
                    .pr(px(8.))
                    .gap(px(8.))
                    .child(
                        img("images/logo.png")
                            .size(px(18.))
                            .flex_none()
                            .rounded(px(4.)),
                    )
                    .child(div().text_size(px(theme::FONT_SM)).child("PatchOpsIII")),
            )
            .child(
                div()
                    .id("check-updates")
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .h(px(20.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .text_size(px(11.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::muted())
                    .when(!can_act, |this| this.opacity(0.7).cursor_not_allowed())
                    .when(can_act, |this| {
                        this.cursor_pointer()
                            .hover(|style| style.bg(theme::white(0.07)).text_color(theme::text()))
                            .on_click(self.stage_click(
                                cx,
                                "Check for updates",
                                "/api/update-check",
                                json!({}),
                            ))
                    })
                    .child(version)
                    .child(icon(Glyph::Refresh, 12., theme::muted())),
            )
            .child(
                chrome::drag_region("titlebar-spacer", drag, frame)
                    .flex_1()
                    .h_full(),
            )
            .child(
                chrome::drag_region("titlebar-status", drag, frame)
                    .flex()
                    .items_center()
                    .h_full()
                    .px(px(8.))
                    .child(self.connection_badge()),
            )
            .child(
                Btn::new("reload-values", "Reload Values")
                    .tiny()
                    .disabled(self.busy || !self.connected)
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                        view.load_inputs(window, cx);
                        cx.notify();
                    })),
            )
            .child(div().w(px(8.)).h_full().flex_none())
            .child(
                Btn::new("refresh", if self.busy { "Working…" } else { "Refresh" })
                    .tiny()
                    .icon(Glyph::Refresh)
                    .disabled(self.busy)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.message = "Connecting to the local service…".into();
                        view.send("/api/status", None, cx);
                    })),
            )
            .child(
                chrome::drag_region("titlebar-end", drag, frame)
                    .flex_none()
                    .h_full()
                    .w(px(10.)),
            )
            .children(chrome::caption_buttons(frame, window))
    }

    fn connection_badge(&self) -> impl IntoElement {
        let (tone, label) = if self.connected {
            (theme::ok(), "Connected")
        } else {
            (theme::danger(), "Offline")
        };
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(theme::FONT_XS))
            .font_weight(FontWeight::BOLD)
            .text_color(tone)
            .child(div().size(px(8.)).rounded_full().bg(tone))
            .child(label)
    }

    /// `.directory-row`.
    fn directory_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let draft = self.text("directory", cx);
        let draft = draft.trim();
        let dirty = !draft.is_empty() && draft != self.string("/gameDir");
        div()
            .flex()
            .items_end()
            .gap(px(10.))
            .flex_none()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(6.))
                    .child(components::field_label("Game Directory:"))
                    .child(self.field("directory")),
            )
            .when(dirty, |row| {
                row.child(
                    Btn::new("use-directory", "Use Directory")
                        .disabled(!self.can_act())
                        .on_click(
                            cx.listener(|view, _: &ClickEvent, _, cx| view.stage_directory(cx)),
                        ),
                )
            })
            .child(
                Btn::new("browse-directory", "Browse...")
                    .icon(Glyph::FolderOpen)
                    .disabled(self.busy)
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                        view.choose_directory("directory", window, cx)
                    })),
            )
            .child(
                self.action(cx, "launch", "Launch Game", "/api/launch", json!({}))
                    .icon(Glyph::Play)
                    .primary(),
            )
    }

    /// `.error-strip`.
    fn error_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .flex_none()
            .gap(px(8.))
            .min_h(px(34.))
            .px(px(12.))
            .py(px(6.))
            .border_1()
            .border_color(theme::danger_alpha(0.38))
            .rounded(px(9.))
            .bg(theme::danger_alpha(0.12))
            .text_color(theme::danger_text())
            .child(icon(Glyph::Alert, 16., theme::danger_text()))
            .child(div().flex_1().min_w_0().child(self.message.clone()))
            .child(
                div()
                    .id("dismiss-error")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(26.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::white(0.12)))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.failed = false;
                        cx.notify();
                    }))
                    .child(icon(Glyph::Close, 15., theme::danger_text())),
            )
    }

    /// `.nav-panel`.
    fn nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(7.))
            .w(px(theme::NAV_WIDTH))
            .p(px(10.))
            .border_1()
            .border_color(theme::border())
            .rounded(theme::radius())
            .bg(theme::white(0.045))
            .children(Page::ALL.into_iter().enumerate().map(|(index, page)| {
                let active = self.page == page;
                let color = if active {
                    theme::text()
                } else {
                    theme::muted_strong()
                };
                div()
                    .id(("nav", index))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .min_h(px(theme::ROW_HEIGHT))
                    .px(px(12.))
                    .border_1()
                    .border_color(if active {
                        theme::accent_alpha(0.22)
                    } else {
                        theme::white(0.)
                    })
                    .rounded(theme::radius_control())
                    .bg(if active {
                        linear_gradient(
                            135.,
                            linear_color_stop(theme::accent_alpha(0.22), 0.),
                            linear_color_stop(theme::white(0.07), 1.),
                        )
                    } else {
                        solid_background(theme::white(0.))
                    })
                    .text_size(px(theme::FONT_MD))
                    .font_weight(FontWeight::BOLD)
                    .text_color(color)
                    .cursor_pointer()
                    .when(!active, |this| {
                        this.hover(|style| style.bg(theme::white(0.06)).text_color(theme::text()))
                    })
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.page = page;
                        view.content_scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    }))
                    .child(icon(page.glyph(), 18., color))
                    .child(div().truncate().child(page.label()))
            }))
    }

    /// `.log-panel`: the last lines of the backend's activity log.
    fn log_panel(&self, height: Pixels, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.visible_logs();
        let status_color = if self.failed {
            theme::warning()
        } else {
            theme::muted()
        };
        div()
            .flex()
            .flex_col()
            .flex_none()
            .h(height)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .gap(px(12.))
                    .mb(px(8.))
                    .child(
                        div()
                            .text_size(px(theme::FONT_LG))
                            .font_weight(FontWeight::BOLD)
                            .child("Activity Log"),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(theme::FONT_XS))
                            .text_color(status_color)
                            .child(self.status_line()),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(10.))
                    .border_1()
                    .border_color(theme::border())
                    .rounded(theme::radius())
                    .bg(theme::panel())
                    .child(
                        div()
                            .relative()
                            .size_full()
                            .border_1()
                            .border_color(theme::white(0.))
                            .rounded(px(9.))
                            .bg(theme::log_surface())
                            .child(
                                div()
                                    .id("log-scroll")
                                    .size_full()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.log_scroll)
                                    .px(px(10.))
                                    .py(px(8.))
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_size(px(theme::FONT_XS))
                                    .when(entries.is_empty(), |this| {
                                        this.text_color(theme::muted()).child("No activity yet.")
                                    })
                                    .children(entries.into_iter().map(|(category, message)| {
                                        let tone = match category.as_str() {
                                            "Success" => theme::ok(),
                                            "Warning" => theme::warning(),
                                            "Error" => theme::danger(),
                                            _ => theme::muted(),
                                        };
                                        div()
                                            .flex()
                                            .gap(px(10.))
                                            .mb(px(4.))
                                            .text_color(theme::text().alpha(0.76))
                                            .child(
                                                div()
                                                    .w(px(72.))
                                                    .flex_none()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(tone)
                                                    .child(category),
                                            )
                                            .child(div().flex_1().min_w_0().child(message))
                                    })),
                            )
                            .vertical_scrollbar(&self.log_scroll),
                    ),
            )
    }

    /// `.startup-screen`, shown until the first status document arrives.
    fn startup_screen(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let failed = self.failed;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .child(img("images/logo.png").size(px(64.)).rounded(px(14.)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(5.))
                    .max_w(px(360.))
                    .text_center()
                    .child(
                        div()
                            .text_size(px(theme::FONT_LG))
                            .font_weight(FontWeight::BOLD)
                            .child(if failed {
                                "Startup took too long"
                            } else {
                                "Opening PatchOpsIII"
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(theme::FONT_SM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::muted())
                            .child(if failed {
                                self.message.clone()
                            } else {
                                "Loading your settings...".to_owned()
                            }),
                    ),
            )
            .when(!failed, |this| {
                this.child(
                    div()
                        .w(px(132.))
                        .h(px(3.))
                        .rounded_full()
                        .bg(theme::white(0.1))
                        .child(div().w(px(55.)).h_full().rounded_full().bg(theme::accent())),
                )
            })
            .when(failed, |this| {
                this.child(
                    Btn::new("retry", "Try Again")
                        .compact()
                        .icon(Glyph::Refresh)
                        .disabled(self.busy)
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                            view.message = "Connecting to the local service…".into();
                            view.failed = false;
                            view.send("/api/status", None, cx);
                        })),
                )
            })
    }
}

impl Render for ControlCenter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.first_frame {
            self.first_frame = true;
            chrome::first_frame(window, cx);
        }
        let ready = !self.state.is_null();
        // `.log-panel` is 180px tall with a 140px floor; give the pages the
        // difference on short windows so the default 820px layout fits.
        let log_height = px((f32::from(window.viewport_size().height) * 0.19).clamp(140., 180.));
        let body = if ready {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .gap(px(theme::APP_GAP))
                .child(self.directory_row(cx))
                .when(self.failed, |this| this.child(self.error_strip(cx)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .gap(px(theme::PANEL_GAP))
                        .child(
                            div()
                                .flex()
                                .flex_1()
                                .min_h_0()
                                .gap(px(theme::PANEL_GAP))
                                .child(self.nav(cx))
                                .child(
                                    div()
                                        .relative()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .id("content-scroll")
                                                .size_full()
                                                .overflow_y_scroll()
                                                .track_scroll(&self.content_scroll)
                                                .flex()
                                                .flex_col()
                                                .gap(px(theme::PANEL_GAP))
                                                .pr(px(8.))
                                                // Content must keep its natural height (flex-shrink
                                                // would squash it into the viewport and clip the
                                                // cards instead of scrolling).
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .flex_none()
                                                        .child(self.page_view(cx)),
                                                ),
                                        )
                                        .vertical_scrollbar(&self.content_scroll),
                                ),
                        )
                        .child(self.log_panel(log_height, cx)),
                )
        } else {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(self.startup_screen(cx))
        };
        let frame = chrome::Frame::read(window);
        let root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(linear_gradient(
                180.,
                linear_color_stop(theme::bg_deep(), 0.),
                linear_color_stop(theme::bg(), 1.),
            ))
            .font(theme::ui_font())
            .text_color(theme::text())
            .text_size(px(theme::FONT_MD))
            .child(self.header(frame, window, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .p(px(theme::APP_PAD))
                    .child(body),
            )
            .children(
                self.confirm_modal(cx)
                    .map(|modal| frame.round_overlay(modal)),
            )
            .children(self.depot_modal(cx).map(|modal| frame.round_overlay(modal)));
        // Rounded corners and the 1px outline of a client-decorated window.
        frame.frame(root)
    }
}
