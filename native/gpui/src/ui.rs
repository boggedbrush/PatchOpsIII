use crate::backend::{Backend, Request};
use gpui::{prelude::*, *};
use gpui_component::{
    Disableable,
    button::{Button, ButtonVariants},
    input::{Input, InputState},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const PAGES: [&str; 7] = [
    "Dashboard",
    "T7 Patch",
    "EXE Swapper",
    "Enhanced",
    "Graphics",
    "DXVK",
    "Tools",
];

pub struct ControlCenter {
    backend: Backend,
    state: Value,
    page: usize,
    busy: bool,
    connected: bool,
    message: String,
    failed: bool,
    last_refresh: Instant,
    inputs: BTreeMap<&'static str, Entity<InputState>>,
    pending: Option<(String, Request)>,
}

impl ControlCenter {
    pub fn new(backend: Backend, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let inputs = [
            ("directory", "Game directory"),
            ("gamertag", "Gamertag (up to 20 characters)"),
            ("password", "Network password"),
            ("color", "Color code, e.g. ^1"),
            ("dump", "BO3 Enhanced source directory"),
            ("fps", "0–1000"),
            ("fov", "65–120"),
            ("resolution", "1920x1080"),
            ("refresh", "Refresh rate"),
            ("render", "Render resolution %"),
            ("latency", "0–4"),
            ("vram", "75–100"),
            ("dxvk-threads", "0–64"),
            ("dxvk-fps", "0–360"),
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
        let mut view = Self {
            backend,
            state: Value::Null,
            page: 0,
            busy: false,
            connected: false,
            message: "Connecting to the local service…".into(),
            failed: false,
            last_refresh: Instant::now(),
            inputs,
            pending: None,
        };
        view.send("/api/status", None, cx);
        // Drain replies and periodically refresh on GPUI's foreground executor.
        // The only blocking work lives on the dedicated HTTP worker.
        cx.spawn_in(window, async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if entity
                    .update_in(cx, |view, window, cx| {
                        while let Ok(reply) = view.backend.replies.try_recv() {
                            view.busy = false;
                            if reply.failed {
                                view.failed = true;
                                if reply.is_status {
                                    view.connected = false;
                                }
                                view.message = reply.message;
                            } else {
                                let initial = view.state.is_null();
                                let reconnected = !view.connected;
                                if let Some(state) = reply.state {
                                    view.state = state;
                                    view.connected = true;
                                }
                                if initial {
                                    view.load_inputs(window, cx);
                                }
                                if !reply.is_status
                                    || reconnected
                                    || view.message.starts_with("Connecting")
                                {
                                    view.failed = false;
                                    view.message = reply.message;
                                }
                            }
                            view.last_refresh = Instant::now();
                            cx.notify();
                        }
                        if !view.busy
                            && view.pending.is_none()
                            && view.last_refresh.elapsed() >= Duration::from_secs(5)
                        {
                            view.send("/api/status", None, cx);
                        }
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
                self.last_refresh = Instant::now();
            }
            Err(_) => {
                self.failed = true;
                self.connected = false;
                self.message = "Local service worker stopped. Restart GPUI.".into();
            }
        }
        cx.notify();
    }

    fn stage(&mut self, label: String, path: String, body: Value, cx: &mut Context<Self>) {
        self.pending = Some((
            label,
            Request {
                path,
                body: Some(body),
            },
        ));
        cx.notify();
    }

    fn text(&self, key: &str, cx: &App) -> String {
        self.inputs[key].read(cx).value().to_string()
    }
    fn load_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (key, pointer) in [
            ("directory", "/gameDir"),
            ("gamertag", "/t7/plainName"),
            ("color", "/t7/colorCode"),
            ("dump", "/enhanced/dumpSource"),
            ("fps", "/graphics/maxFps"),
            ("fov", "/graphics/fov"),
            ("resolution", "/graphics/resolution"),
            ("refresh", "/graphics/refreshRate"),
            ("render", "/graphics/renderResolution"),
            ("latency", "/advanced/maxFrameLatency"),
            ("vram", "/advanced/vramTarget"),
            ("dxvk-threads", "/dxvk/settings/numCompilerThreads"),
            ("dxvk-fps", "/dxvk/settings/maxFrameRate"),
            ("dxvk-latency", "/dxvk/settings/maxFrameLatency"),
        ] {
            let value = self.value(pointer);
            self.inputs[key].update(cx, |state, cx| {
                state.set_value(if value == "—" { String::new() } else { value }, window, cx)
            });
        }
    }
    fn flag(&self, pointer: &str) -> bool {
        self.state
            .pointer(pointer)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }
    fn value(&self, pointer: &str) -> String {
        match self.state.pointer(pointer) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n
                .as_f64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| n.to_string()),
            Some(Value::Null) | None => "—".into(),
            Some(v) => v.to_string(),
        }
    }

    fn action(
        &self,
        id: String,
        label: String,
        path: &str,
        body: Value,
        cx: &mut Context<Self>,
    ) -> Button {
        let path = path.to_owned();
        Button::new(SharedString::from(id))
            .label(label.clone())
            .disabled(self.busy || !self.connected || self.pending.is_some())
            .on_click(cx.listener(move |view, _, _, cx| {
                view.stage(label.clone(), path.clone(), body.clone(), cx)
            }))
    }

    fn toggle(
        &self,
        label: &str,
        pointer: &str,
        path: &str,
        key: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self.flag(pointer);
        let body = json!({key: !current});
        div()
            .flex()
            .justify_between()
            .items_center()
            .gap_4()
            .child(format!("{label}: {}", if current { "On" } else { "Off" }))
            .child(self.action(
                format!("toggle-{pointer}"),
                format!("{} {label}", if current { "Disable" } else { "Enable" }),
                path,
                body,
                cx,
            ))
            .into_any_element()
    }

    fn config_toggle(
        &self,
        label: &str,
        pointer: &str,
        key: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self.flag(pointer);
        div().flex().justify_between().items_center().gap_4()
            .child(format!("{label}: {}", if current { "On" } else { "Off" }))
            .child(self.action(format!("config-{key}"), format!("{} {label}", if current { "Disable" } else { "Enable" }), "/api/config",
                json!({"key": key, "value": if current {0} else {1}, "comment": "Managed by PatchOpsIII"}), cx)).into_any_element()
    }

    fn field(&self, label: &str, key: &str) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(label.to_owned())
            .child(Input::new(&self.inputs[key]).disabled(self.busy))
            .into_any_element()
    }

    fn numeric_field(
        &self,
        label: &str,
        input: &'static str,
        config: &'static str,
        min: i64,
        max: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .items_end()
            .gap_3()
            .child(div().flex_1().child(self.field(label, input)))
            .child(
                Button::new(SharedString::from(format!("apply-{input}")))
                    .label("Apply")
                    .disabled(self.busy || !self.connected || self.pending.is_some())
                    .on_click(cx.listener(move |view, _, _, cx| {
                        match view.text(input, cx).parse::<i64>() {
                            Ok(value) if (min..=max).contains(&value) => view.stage(
                                format!("Set {config} to {value}"),
                                "/api/config".into(),
                                json!({"key": config, "value": value}),
                                cx,
                            ),
                            _ => {
                                view.message = format!("Enter a whole number from {min} to {max}.");
                                view.failed = true;
                                cx.notify();
                            }
                        }
                    })),
            )
            .into_any_element()
    }

    fn choose_directory(&mut self, input: &'static str, window: &Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select a folder".into()),
        });
        cx.spawn_in(window, async move |entity, cx| match paths.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.first() {
                    let value = path.to_string_lossy().into_owned();
                    let _ = entity.update_in(cx, |view, window, cx| {
                        view.inputs[input]
                            .update(cx, |state, cx| state.set_value(value, window, cx));
                    });
                }
            }
            Ok(Ok(None)) => {}
            _ => {
                let _ = entity.update(cx, |view, cx| {
                    view.message = "Folder picker unavailable. Enter the path directly.".into();
                    view.failed = true;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn folder_field(&self, label: &str, key: &'static str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_end()
            .gap_3()
            .child(div().flex_1().child(self.field(label, key)))
            .child(
                Button::new(SharedString::from(format!("browse-{key}")))
                    .label("Browse…")
                    .disabled(self.busy)
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.choose_directory(key, window, cx)
                    })),
            )
            .into_any_element()
    }

    fn dashboard(&self, cx: &mut Context<Self>) -> AnyElement {
        panel("Game installation")
            .child(format!("Current directory: {}", self.value("/gameDir")))
            .child(format!(
                "Platform: {}   ·   Version: {}",
                self.value("/platform"),
                self.value("/appVersion")
            ))
            .child(format!(
                "Executable: {}   ·   {}",
                self.value("/exeSwap/displayLabel"),
                self.value("/exeSwap/integrityMessage")
            ))
            .child(self.folder_field("Black Ops III directory", "directory", cx))
            .child(
                Button::new("save-directory")
                    .label("Use this directory")
                    .primary()
                    .disabled(self.busy || !self.connected || self.pending.is_some())
                    .on_click(cx.listener(|view, _, _, cx| {
                        let path = view.text("directory", cx);
                        if path.trim().is_empty() {
                            view.message = "Select or enter a directory first.".into();
                            view.failed = true;
                            cx.notify();
                            return;
                        }
                        view.stage(
                            format!("Use directory {path}"),
                            "/api/game-directory".into(),
                            json!({"path": path}),
                            cx,
                        );
                    })),
            )
            .child(div().mt_4().child("Launch profiles"))
            .children(
                self.state["launchProfiles"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|profile| {
                        let label = profile["label"].as_str().unwrap_or("Profile").to_owned();
                        let option = profile["option"].as_str().unwrap_or("").to_owned();
                        let id = profile["id"].as_str().unwrap_or("profile").to_owned();
                        div()
                            .flex()
                            .gap_3()
                            .items_center()
                            .child(self.action(
                                format!("profile-{id}"),
                                format!(
                                    "{label}{}",
                                    if profile["active"] == true {
                                        " · Active"
                                    } else {
                                        ""
                                    }
                                ),
                                "/api/launch-options",
                                json!({"options": option}),
                                cx,
                            ))
                            .when(
                                profile["subscribed"].is_boolean()
                                    && id != "default"
                                    && id != "offline",
                                |row| {
                                    row.child(self.action(
                                        format!("workshop-{id}"),
                                        "Open Workshop".into(),
                                        "/api/workshop-install",
                                        json!({"profileId": id}),
                                        cx,
                                    ))
                                },
                            )
                    }),
            )
            .into_any_element()
    }

    fn t7(&self, cx: &mut Context<Self>) -> AnyElement {
        panel("T7 Patch")
            .child(format!("Installed: {}   ·   Current gamertag: {}", self.flag("/t7/installed"), self.value("/t7/plainName")))
            .child(div().flex().gap_3()
                .child(self.action("t7-install".into(), "Install / update T7 Patch".into(), "/api/t7-install", json!({}), cx))
                .child(self.action("t7-uninstall".into(), "Uninstall T7 Patch".into(), "/api/t7-uninstall", json!({}), cx)))
            .child(self.field("Gamertag", "gamertag"))
            .child(self.field("Gamertag color", "color"))
            .child(Button::new("save-gamertag").label("Save gamertag").disabled(self.busy || !self.connected || self.pending.is_some())
                .on_click(cx.listener(|view, _, _, cx| view.stage("Update T7 gamertag".into(), "/api/t7-config".into(), json!({"gamertag": view.text("gamertag", cx), "colorCode": view.text("color", cx)}), cx))))
            .child(self.field("Network password (blank clears it)", "password"))
            .child(Button::new("save-password").label("Save network password").disabled(self.busy || !self.connected || self.pending.is_some())
                .on_click(cx.listener(|view, _, _, cx| view.stage("Update T7 network password".into(), "/api/t7-config".into(), json!({"networkPassword": view.text("password", cx)}), cx))))
            .child(self.toggle("Friends only", "/t7/friendsOnly", "/api/t7-config", "friendsOnly", cx)).into_any_element()
    }

    fn exe(&self, cx: &mut Context<Self>) -> AnyElement {
        panel("Executable profiles")
            .child(self.value("/exeSwap/displayLabel"))
            .child(self.value("/exeSwap/integrityMessage"))
            .child(format!("SHA-256: {}", self.value("/exeSwap/executableHash")))
            .child("Compatible installs may require a Steam depot download. Instructions appear in the operation result below.")
            .children([("compatible", "Install compatible build"), ("current", "Restore current Steam build"), ("enhanced", "Restore Enhanced build")].into_iter().map(|(id, label)|
                self.action(format!("exe-{id}"), label.into(), &format!("/api/exe-swap/{id}"), json!({}), cx))).into_any_element()
    }

    fn enhanced(&self, cx: &mut Context<Self>) -> AnyElement {
        panel("BO3 Enhanced")
            .child(format!(
                "Installed: {}   ·   Backup status: {}",
                self.flag("/enhanced/installed"),
                self.value("/enhanced/backupStatus")
            ))
            .child(self.folder_field("Enhanced source directory", "dump", cx))
            .children(
                [
                    ("validate", "Validate source"),
                    ("install", "Install Enhanced"),
                ]
                .into_iter()
                .map(|(id, label)| {
                    Button::new(SharedString::from(format!("enhanced-{id}")))
                        .label(label)
                        .disabled(self.busy || !self.connected || self.pending.is_some())
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.stage(
                                label.into(),
                                format!("/api/enhanced-{id}"),
                                json!({"dumpSource": view.text("dump", cx)}),
                                cx,
                            )
                        }))
                }),
            )
            .child(self.action(
                "enhanced-uninstall".into(),
                "Uninstall Enhanced".into(),
                "/api/enhanced-uninstall",
                json!({}),
                cx,
            ))
            .into_any_element()
    }

    fn graphics(&self, cx: &mut Context<Self>) -> AnyElement {
        panel("Graphics & performance")
            .child(format!("Current: {} · {} Hz · FOV {} · FPS cap {}", self.value("/graphics/resolution"), self.value("/graphics/refreshRate"), self.value("/graphics/fov"), self.value("/graphics/maxFps")))
            .children(self.state["presets"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|name|
                self.action(format!("preset-{name}"), format!("Apply {name}"), "/api/presets/apply", json!({"name": name}), cx)))
            .child(self.numeric_field("FPS cap (0 disables the limiter)", "fps", "MaxFPS", 0, 1000, cx))
            .child(self.numeric_field("Field of view", "fov", "FOV", 65, 120, cx))
            .child(self.numeric_field("Refresh rate", "refresh", "RefreshRate", 1, 1000, cx))
            .child(self.numeric_field("Render resolution %", "render", "ResolutionPercent", 50, 200, cx))
            .child(self.field("Resolution (WIDTHxHEIGHT)", "resolution"))
            .child(Button::new("save-resolution").label("Apply resolution").disabled(self.busy || !self.connected || self.pending.is_some())
                .on_click(cx.listener(|view, _, _, cx| {
                    let value = view.text("resolution", cx);
                    let valid = value.split_once('x').is_some_and(|(w, h)| [w, h].iter().all(|n| n.parse::<u32>().is_ok_and(|n| (320..=16384).contains(&n))));
                    if valid { view.stage(format!("Set resolution to {value}"), "/api/config".into(), json!({"key": "WindowSize", "value": value}), cx); }
                    else { view.message = "Enter resolution as WIDTHxHEIGHT, e.g. 1920x1080.".into(); view.failed = true; cx.notify(); }
                })))
            .child(div().flex().gap_2().children([(0, "Windowed"), (1, "Fullscreen"), (2, "Borderless")].into_iter().map(|(mode, label)| self.action(format!("display-{mode}"), label.into(), "/api/config", json!({"key": "FullScreenMode", "value": mode}), cx))))
            .child(self.config_toggle("V-Sync", "/graphics/vsync", "Vsync", cx))
            .child(self.config_toggle("FPS counter", "/graphics/drawFps", "DrawFPS", cx))
            .child(self.config_toggle("Frame smoothing", "/advanced/smoothFramerate", "SmoothFramerate", cx))
            .child(self.action("unlock-graphics".into(), "Toggle hidden graphics options".into(), "/api/config", json!({"key": "RestrictGraphicsOptions", "value": if self.flag("/advanced/unlockOptions") {1} else {0}}), cx))
            .child(self.action("reduce-cpu".into(), "Toggle reduced CPU pressure".into(), "/api/config", json!({"key": "SerializeRender", "value": if self.flag("/advanced/reduceCpu") {0} else {2}}), cx))
            .child(self.numeric_field("Maximum frame latency", "latency", "MaxFrameLatency", 0, 4, cx))
            .child(self.field("VRAM target %", "vram"))
            .child(Button::new("save-vram").label("Apply VRAM target").disabled(self.busy || !self.connected || self.pending.is_some())
                .on_click(cx.listener(|view, _, _, cx| {
                    match view.text("vram", cx).parse::<u32>() {
                        Ok(target) if (75..=100).contains(&target) => view.stage(format!("Set VRAM target to {target}%"), "/api/vram-target".into(), json!({"limited": target < 100, "target": target}), cx),
                        _ => { view.message = "VRAM target must be a whole number from 75 to 100.".into(); view.failed = true; cx.notify(); }
                    }
                })))
            .child(self.toggle("Skip intro", "/qol/intro", "/api/intro-skip", "enabled", cx))
            .child(self.toggle("Skip all intros", "/qol/allIntros", "/api/all-intros-skip", "enabled", cx))
            .child(self.toggle("Modern DirectX compiler", "/qol/d3dcompiler", "/api/d3dcompiler", "enabled", cx))
            .child(self.toggle("Read-only config", "/advanced/configReadonly", "/api/config-readonly", "enabled", cx)).into_any_element()
    }

    fn dxvk(&self, cx: &mut Context<Self>) -> AnyElement {
        let settings = self
            .state
            .pointer("/dxvk/settings")
            .cloned()
            .unwrap_or(json!({}));
        panel("DXVK-GPLAsync")
            .child(format!("Installed: {}", self.flag("/dxvk/installed")))
            .child("Install uses the settings reported by the service, or backend defaults for a new installation.")
            .child(format!("Current settings: {settings}"))
            .child(self.action("dxvk-install".into(), "Install DXVK".into(), "/api/dxvk-install", settings.clone(), cx))
            .child(self.action("dxvk-uninstall".into(), "Uninstall DXVK".into(), "/api/dxvk-uninstall", json!({}), cx))
            .children([("Async compilation", "enableAsync"), ("GPL async cache", "gplAsyncCache"), ("DXVK HUD", "hudEnabled")].into_iter().map(|(label, key)| {
                let mut next = settings.clone();
                next[key] = json!(!settings[key].as_bool().unwrap_or(false));
                self.action(format!("dxvk-{key}"), format!("Toggle {label}"), "/api/dxvk-config", next, cx)
            }))
            .children([("Compiler threads", "dxvk-threads", "numCompilerThreads", 64), ("FPS cap", "dxvk-fps", "maxFrameRate", 360), ("Frame latency", "dxvk-latency", "maxFrameLatency", 16)].into_iter().map(|(label, input, setting, max)| {
                div().flex().items_end().gap_3().child(div().flex_1().child(self.field(label, input)))
                    .child(Button::new(SharedString::from(format!("apply-{input}"))).label("Apply").disabled(self.busy || !self.connected || self.pending.is_some())
                        .on_click(cx.listener(move |view, _, _, cx| {
                            match view.text(input, cx).parse::<u32>() {
                                Ok(value) if value <= max => { let mut settings = view.state["dxvk"]["settings"].clone(); settings[setting] = json!(value); view.stage(format!("Set DXVK {label} to {value}"), "/api/dxvk-config".into(), settings, cx); }
                                _ => { view.message = format!("Enter a whole number from 0 to {max}."); view.failed = true; cx.notify(); }
                            }
                        })))
            }))
            .children(["True", "False", "Auto"].into_iter().map(|mode| { let mut next = settings.clone(); next["tearFree"] = json!(mode); self.action(format!("tearfree-{mode}"), format!("Tear-free: {mode}"), "/api/dxvk-config", next, cx) })).into_any_element()
    }

    fn tools(&self, cx: &mut Context<Self>) -> AnyElement {
        panel("Maintenance")
            .child(format!("Log file: {}", self.value("/logPath")))
            .children(
                [
                    ("/api/logs/clear", "Clear logs"),
                    ("/api/mod-files/clear", "Clear cached mod files"),
                    ("/api/reset-stock", "Reset game to stock"),
                    ("/api/update-check", "Check for updates"),
                ]
                .into_iter()
                .map(|(path, label)| self.action(path.into(), label.into(), path, json!({}), cx)),
            )
            .child(self.action(
                "channel-stable".into(),
                "Use stable channel".into(),
                "/api/release-channel",
                json!({"channel": "stable"}),
                cx,
            ))
            .child(self.action(
                "channel-beta".into(),
                "Use beta channel".into(),
                "/api/release-channel",
                json!({"channel": "beta"}),
                cx,
            ))
            .into_any_element()
    }
}

fn panel(title: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .rounded_lg()
        .border_1()
        .border_color(rgb(0x343438))
        .bg(rgb(0x17171b))
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::BOLD)
                .child(title.to_owned()),
        )
}

impl Render for ControlCenter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.page {
            0 => self.dashboard(cx),
            1 => self.t7(cx),
            2 => self.exe(cx),
            3 => self.enhanced(cx),
            4 => self.graphics(cx),
            5 => self.dxvk(cx),
            _ => self.tools(cx),
        };
        let confirmation = self.pending.as_ref().map(|(label, _)| {
            panel("Confirm operation")
                .child(label.clone())
                .child(format!("Target: {}", self.value("/gameDir")))
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(Button::new("confirm").label("Confirm").primary().on_click(
                            cx.listener(|view, _, _, cx| {
                                if let Some((label, request)) = view.pending.take() {
                                    view.message = format!("Running: {label}…");
                                    view.failed = false;
                                    view.send(&request.path, request.body, cx);
                                }
                            }),
                        ))
                        .child(Button::new("cancel").label("Cancel").on_click(cx.listener(
                            |view, _, _, cx| {
                                view.pending = None;
                                cx.notify();
                            },
                        ))),
                )
                .into_any_element()
        });
        div()
            .flex()
            .size_full()
            .bg(rgb(0x0d0d0f))
            .text_color(rgb(0xf6f6f6))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(200.))
                    .p_4()
                    .gap_3()
                    .border_r_1()
                    .border_color(rgb(0x343438))
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .child("PATCHOPS III"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xff4545))
                            .child("GPUI evaluation"),
                    )
                    .children(PAGES.into_iter().enumerate().map(|(index, label)| {
                        Button::new(SharedString::from(format!("nav-{index}")))
                            .label(label)
                            .when(index == self.page, |button| button.primary())
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.page = index;
                                cx.notify();
                            }))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .p_5()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::BOLD)
                                    .child(PAGES[self.page]),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("load-settings")
                                            .label("Load current values")
                                            .disabled(self.busy || !self.connected)
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.load_inputs(window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("refresh")
                                            .label(if self.busy { "Working…" } else { "Refresh" })
                                            .disabled(self.busy)
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                view.message =
                                                    "Connecting to the local service…".into();
                                                view.send("/api/status", None, cx);
                                            })),
                                    )
                                    .child(self.action(
                                        "launch".into(),
                                        "Launch game".into(),
                                        "/api/launch",
                                        json!({}),
                                        cx,
                                    )),
                            ),
                    )
                    .children(confirmation)
                    .child(
                        div()
                            .id("content-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(content),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(if self.failed {
                                rgb(0xff9f0a)
                            } else {
                                rgb(0xb6b6bb)
                            })
                            .child(self.message.clone()),
                    )
                    .child(
                        div()
                            .id("log-scroll")
                            .h(px(130.))
                            .overflow_y_scroll()
                            .bg(rgb(0x050505))
                            .rounded_lg()
                            .p_3()
                            .text_sm()
                            .children(
                                self.state["logs"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .rev()
                                    .take(80)
                                    .map(|entry| {
                                        div()
                                            .text_color(if entry["category"] == "Error" {
                                                rgb(0xff4545)
                                            } else {
                                                rgb(0xb6b6bb)
                                            })
                                            .child(entry["line"].as_str().unwrap_or("").to_owned())
                                    }),
                            ),
                    ),
            )
    }
}
