//! Graphics settings and DXVK. Both write `config.ini` / `dxvk.conf` through
//! the local service; every write asks for confirmation first.
use super::components::{
    Btn, Glyph, bare_panel, chip, column, columns, handler, icon, inline_pill, segmented, switch,
    text_xs,
};
use super::progress::Operation;
use super::{ControlCenter, GraphicsTab, dxvk_recommended, theme};
use gpui::{prelude::*, *};
use gpui_component::slider::Slider;
use serde_json::{Value, json};

const DISPLAY_MODES: [(i64, &str); 3] = [(0, "Windowed"), (1, "Fullscreen"), (2, "Borderless")];

fn dxvk_none() -> Value {
    json!({
        "enableAsync": true,
        "gplAsyncCache": false,
        "numCompilerThreads": 0,
        "maxFrameRate": 0,
        "maxFrameLatency": 0,
        "tearFree": "Auto",
        "hudEnabled": false,
    })
}

/// `.graphics-quick-grid .setting-row`: a bordered tile with a label and a control.
fn quick_row(label: &str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .flex_1()
        .min_w(px(300.))
        .min_h(px(50.))
        .px(px(14.))
        .py(px(6.))
        .border_1()
        .border_color(theme::border())
        .rounded(theme::radius_card())
        .bg(theme::white(0.04))
        .child(
            div()
                .w(px(104.))
                .flex_none()
                .text_size(px(theme::FONT_MD))
                .child(label.to_owned()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap(px(8.))
                .flex_1()
                .min_w_0()
                .child(control),
        )
}

/// A row inside a settings group or the advanced drawer.
fn group_row(label: &str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .min_h(px(40.))
        .px(px(14.))
        .py(px(6.))
        .child(
            div()
                .min_w(px(110.))
                .flex_none()
                .text_size(px(theme::FONT_MD))
                .child(label.to_owned()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.))
                .flex_1()
                .min_w_0()
                .child(control),
        )
}

/// `.graphics-settings-group`: a titled stack of rows separated by hairlines.
fn group(title: &str, rows: Vec<Div>) -> Div {
    let count = rows.len();
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .overflow_hidden()
        .border_1()
        .border_color(theme::border())
        .rounded(theme::radius_card())
        .bg(theme::white(0.03))
        .child(
            div()
                .px(px(14.))
                .pt(px(12.))
                .pb(px(6.))
                .font_weight(FontWeight::BOLD)
                .child(title.to_owned()),
        )
        .children(rows.into_iter().enumerate().map(|(index, row)| {
            row.when(index + 1 < count, |row| {
                row.border_b_1().border_color(theme::border())
            })
        }))
}

impl ControlCenter {
    /// An input with an `Apply` button that stages a whole-number setting.
    fn number_control(
        &self,
        cx: &mut Context<Self>,
        input: &'static str,
        config: &'static str,
        min: i64,
        max: i64,
    ) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .flex_1()
            .min_w_0()
            .child(div().flex_1().min_w_0().child(self.field(input)))
            .child(
                Btn::new(SharedString::from(format!("apply-{input}")), "Apply")
                    .compact()
                    .disabled(!self.can_act())
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        match view.text(input, cx).trim().parse::<i64>() {
                            Ok(value) if (min..=max).contains(&value) => view.stage(
                                format!("Set {config} to {value}"),
                                "/api/config".into(),
                                ControlCenter::config_body(config, json!(value)),
                                cx,
                            ),
                            _ => view
                                .reject(format!("Enter a whole number from {min} to {max}."), cx),
                        }
                    })),
            )
    }

    /// A slider with its current value and an optional `Apply` button.
    fn slider_control(
        &self,
        cx: &mut Context<Self>,
        key: &'static str,
        apply: Option<(&'static str, &'static str)>,
    ) -> Div {
        let value = self.slider_value(key, cx);
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .flex_1()
            .min_w(px(160.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w(px(100.))
                    .child(Slider::new(&self.sliders[key]).disabled(self.busy)),
            )
            .child(
                div()
                    .w(px(36.))
                    .flex_none()
                    .text_right()
                    .font_weight(FontWeight::BOLD)
                    .child(value.to_string()),
            )
            .when_some(apply, |this, (config, label)| {
                this.child(
                    Btn::new(SharedString::from(format!("apply-{key}")), "Apply")
                        .compact()
                        .disabled(!self.can_act())
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            let value = view.slider_value(key, cx);
                            view.stage(
                                format!("Set {label} to {value}"),
                                "/api/config".into(),
                                ControlCenter::config_body(config, json!(value)),
                                cx,
                            );
                        })),
                )
            })
    }

    /// A switch that writes `value` to a `config.ini` key once confirmed.
    fn config_switch(
        &self,
        cx: &mut Context<Self>,
        id: &'static str,
        label: &str,
        current: bool,
        key: &str,
        next_value: i64,
    ) -> Div {
        group_row(
            label,
            switch(
                id,
                current,
                self.can_act(),
                self.stage_click(
                    cx,
                    format!("{} {label}", if current { "Disable" } else { "Enable" }),
                    "/api/config",
                    ControlCenter::config_body(key, json!(next_value)),
                ),
            ),
        )
    }

    fn graphics_quick_grid(&self, cx: &mut Context<Self>) -> Div {
        let can_act = self.can_act();
        let presets: Vec<String> = self.state["presets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let display_mode = self.int("/graphics/displayMode", 1);
        let preset_chips =
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .children(presets.into_iter().enumerate().map(|(index, name)| {
                    let click = self.stage_click(
                        cx,
                        format!("Apply preset {name}"),
                        "/api/presets/apply",
                        json!({"name": name}),
                    );
                    chip(("preset", index), name, None, false, can_act, click)
                }));
        let modes = segmented(
            DISPLAY_MODES
                .iter()
                .map(|(mode, label)| {
                    (
                        SharedString::from(*label),
                        display_mode == *mode,
                        handler(self.stage_click(
                            cx,
                            format!("Set display mode to {label}"),
                            "/api/config",
                            ControlCenter::config_body("FullScreenMode", json!(mode)),
                        )),
                    )
                })
                .collect(),
            can_act,
        )
        .flex_1();
        columns()
            .child(quick_row("Preset", preset_chips))
            .child(quick_row("Display Mode", modes))
            .child(quick_row(
                "Resolution",
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .flex_1()
                    .min_w_0()
                    .child(div().flex_1().min_w_0().child(self.field("resolution")))
                    .child(
                        Btn::new("apply-resolution", "Apply")
                            .compact()
                            .disabled(!can_act)
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                let value = view.text("resolution", cx).trim().to_owned();
                                let valid = value.split_once('x').is_some_and(|(w, h)| {
                                    [w, h].iter().all(|n| {
                                        n.parse::<u32>().is_ok_and(|n| (320..=16384).contains(&n))
                                    })
                                });
                                if valid {
                                    view.stage(
                                        format!("Set resolution to {value}"),
                                        "/api/config".into(),
                                        ControlCenter::config_body("WindowSize", json!(value)),
                                        cx,
                                    );
                                } else {
                                    view.reject(
                                        "Enter resolution as WIDTHxHEIGHT, e.g. 1920x1080.",
                                        cx,
                                    );
                                }
                            })),
                    ),
            ))
            .child(quick_row(
                "Refresh Rate",
                self.number_control(cx, "refresh", "RefreshRate", 1, 240),
            ))
    }

    fn graphics_advanced(&self, cx: &mut Context<Self>) -> Div {
        let open = self.advanced_open;
        let limited = self.flag("/advanced/vramLimited");
        let target = self.int("/advanced/vramTarget", 75).clamp(75, 100);
        let readonly = self.flag("/advanced/configReadonly");
        let unlocked = self.flag("/advanced/unlockOptions");
        let reduce_cpu = self.flag("/advanced/reduceCpu");
        let can_act = self.can_act();
        let rows = div()
            .flex()
            .flex_wrap()
            .border_t_1()
            .border_color(theme::border())
            .child(
                self.config_switch(
                    cx,
                    "smooth-framerate",
                    "Smooth framerate",
                    self.flag("/advanced/smoothFramerate"),
                    "SmoothFramerate",
                    if self.flag("/advanced/smoothFramerate") {
                        0
                    } else {
                        1
                    },
                )
                .flex_1()
                .min_w(px(300.))
                .border_b_1()
                .border_color(theme::border()),
            )
            .child(
                self.config_switch(
                    cx,
                    "unlock-graphics",
                    "Expose hidden graphics",
                    unlocked,
                    "RestrictGraphicsOptions",
                    if unlocked { 1 } else { 0 },
                )
                .flex_1()
                .min_w(px(300.))
                .border_b_1()
                .border_color(theme::border()),
            )
            .child(
                self.config_switch(
                    cx,
                    "reduce-cpu",
                    "Reduce CPU pressure",
                    reduce_cpu,
                    "SerializeRender",
                    if reduce_cpu { 0 } else { 2 },
                )
                .flex_1()
                .min_w(px(300.))
                .border_b_1()
                .border_color(theme::border()),
            )
            .child(
                group_row(
                    "Frame latency",
                    self.number_control(cx, "latency", "MaxFrameLatency", 0, 4),
                )
                .flex_1()
                .min_w(px(300.))
                .border_b_1()
                .border_color(theme::border()),
            )
            .child(
                group_row(
                    "Limit VRAM target",
                    switch(
                        "limit-vram",
                        limited,
                        can_act,
                        self.stage_click(
                            cx,
                            format!(
                                "{} the VRAM target",
                                if limited { "Remove" } else { "Limit" }
                            ),
                            "/api/vram-target",
                            json!({"limited": !limited, "target": target}),
                        ),
                    ),
                )
                .flex_1()
                .min_w(px(300.))
                .border_b_1()
                .border_color(theme::border()),
            )
            .child(
                group_row(
                    "VRAM target %",
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .flex_1()
                        .min_w_0()
                        .child(div().flex_1().min_w_0().child(self.field("vram")))
                        .child(
                            Btn::new("apply-vram", "Apply")
                                .compact()
                                .disabled(!can_act)
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    match view.text("vram", cx).trim().parse::<u32>() {
                                        Ok(target) if (75..=100).contains(&target) => view.stage(
                                            format!("Set VRAM target to {target}%"),
                                            "/api/vram-target".into(),
                                            json!({"limited": target < 100, "target": target}),
                                            cx,
                                        ),
                                        _ => view.reject(
                                            "VRAM target must be a whole number from 75 to 100.",
                                            cx,
                                        ),
                                    }
                                })),
                        ),
                )
                .flex_1()
                .min_w(px(300.))
                .border_b_1()
                .border_color(theme::border()),
            )
            .child(
                group_row(
                    "Lock config.ini",
                    switch(
                        "config-readonly",
                        readonly,
                        can_act,
                        self.stage_click(
                            cx,
                            format!(
                                "{} config.ini read-only mode",
                                if readonly { "Disable" } else { "Enable" }
                            ),
                            "/api/config-readonly",
                            json!({"enabled": !readonly}),
                        ),
                    ),
                )
                .flex_1()
                .min_w(px(300.))
                .border_b_1()
                .border_color(theme::border()),
            );
        div()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_1()
            .border_color(theme::border())
            .rounded(theme::radius_card())
            .bg(theme::white(0.025))
            .child(
                div()
                    .id("advanced-drawer")
                    .flex()
                    .items_center()
                    .justify_between()
                    .min_h(px(46.))
                    .px(px(14.))
                    .cursor_pointer()
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.advanced_open = !view.advanced_open;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().font_weight(FontWeight::BOLD).child("Advanced"))
                            .child(text_xs(
                                "Config, CPU, latency, and VRAM controls",
                                theme::muted(),
                            )),
                    )
                    .child(icon(
                        if open {
                            Glyph::ChevronDown
                        } else {
                            Glyph::ChevronRight
                        },
                        16.,
                        theme::muted(),
                    )),
            )
            .when(open, |this| this.child(rows))
    }

    fn graphics_settings(&self, cx: &mut Context<Self>) -> Div {
        let vsync = self.flag("/graphics/vsync");
        let draw_fps = self.flag("/graphics/drawFps");
        bare_panel(
            "Graphics Settings",
            div()
                .flex()
                .flex_col()
                .gap(px(theme::PANEL_GAP))
                .child(self.graphics_quick_grid(cx))
                .child(
                    columns()
                        .items_start()
                        .child(column(340.).child(group(
                            "Performance",
                            vec![
                                group_row(
                                    "Max FPS",
                                    self.number_control(cx, "fps", "MaxFPS", 0, 1000),
                                ),
                                self.config_switch(
                                    cx,
                                    "vsync",
                                    "Vertical sync",
                                    vsync,
                                    "Vsync",
                                    if vsync { 0 } else { 1 },
                                ),
                                self.config_switch(
                                    cx,
                                    "draw-fps",
                                    "FPS counter",
                                    draw_fps,
                                    "DrawFPS",
                                    if draw_fps { 0 } else { 1 },
                                ),
                            ],
                        )))
                        .child(column(340.).child(group(
                            "Quality",
                            vec![
                                group_row(
                                    "Render Resolution",
                                    self.slider_control(
                                        cx,
                                        "render",
                                        Some(("ResolutionPercent", "render resolution")),
                                    ),
                                ),
                                group_row(
                                    "Field of View",
                                    self.slider_control(cx, "fov", Some(("FOV", "field of view"))),
                                ),
                            ],
                        ))),
                )
                .child(self.graphics_advanced(cx)),
        )
    }

    /// The current DXVK draft with the number inputs and slider folded in.
    fn dxvk_payload(&self, cx: &App) -> Result<Value, String> {
        let parse = |key: &str, label: &str, max: i64| -> Result<i64, String> {
            self.text(key, cx)
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|value| (0..=max).contains(value))
                .ok_or_else(|| format!("{label} must be a whole number from 0 to {max}."))
        };
        let mut payload = self.dxvk_draft.clone();
        payload["numCompilerThreads"] = json!(parse("dxvk-threads", "Compiler threads", 64)?);
        payload["maxFrameLatency"] = json!(parse("dxvk-latency", "Frame latency", 16)?);
        payload["maxFrameRate"] = json!(self.slider_value("dxvk-fps", cx));
        Ok(payload)
    }

    /// Stage a DXVK install/apply after folding the draft controls in.
    fn dxvk_click(
        &self,
        cx: &mut Context<Self>,
        label: &'static str,
        path: &'static str,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        cx.listener(
            move |view, _: &ClickEvent, _, cx| match view.dxvk_payload(cx) {
                Ok(payload) => view.stage(label.into(), path.into(), payload, cx),
                Err(message) => view.reject(message, cx),
            },
        )
    }

    fn dxvk_toggle(
        &self,
        cx: &mut Context<Self>,
        id: &'static str,
        label: &str,
        key: &'static str,
    ) -> Div {
        group_row(
            label,
            switch(
                id,
                self.dxvk_flag(key),
                !self.busy,
                cx.listener(move |view, _: &ClickEvent, _, cx| {
                    let next = !view.dxvk_flag(key);
                    view.dxvk_draft[key] = json!(next);
                    cx.notify();
                }),
            ),
        )
    }

    fn dxvk_view(&self, cx: &mut Context<Self>) -> Div {
        let installed = self.flag("/dxvk/installed");
        let conf = self.flag("/dxvk/confExists");
        let can_act = self.can_act();
        let detected_threads = std::thread::available_parallelism()
            .ok()
            .map(|cores| cores.get().saturating_sub(2).max(1));
        let pill = |label: &str, value: &str, ok: bool| {
            div()
                .flex_1()
                .min_w(px(180.))
                .min_h(px(58.))
                .border_1()
                .border_color(theme::border())
                .rounded(theme::radius_card())
                .bg(theme::white(0.04))
                .child(inline_pill(label.to_owned(), value.to_owned(), ok))
        };
        let control_bar = div()
            .flex()
            .flex_wrap()
            .gap(px(theme::PANEL_GAP))
            .child(pill(
                "Status",
                if installed {
                    "Installed"
                } else {
                    "Not installed"
                },
                installed,
            ))
            .child(pill(
                "dxvk.conf",
                if conf { "Configured" } else { "Missing" },
                conf,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .flex_1()
                    .min_w(px(300.))
                    .min_h(px(58.))
                    .p(px(8.))
                    .border_1()
                    .border_color(theme::border())
                    .rounded(theme::radius_card())
                    .bg(theme::white(0.04))
                    .child(
                        Btn::new("dxvk-install", "Install")
                            .compact()
                            .icon(Glyph::Download)
                            .grow()
                            .disabled(!can_act)
                            .on_click(self.dxvk_click(cx, "Install DXVK", "/api/dxvk-install")),
                    )
                    .child(
                        self.action(
                            cx,
                            "dxvk-uninstall",
                            "Uninstall",
                            "/api/dxvk-uninstall",
                            json!({}),
                        )
                        .compact()
                        .icon(Glyph::Trash)
                        .grow()
                        .disabled(!can_act || !installed),
                    )
                    .child(
                        Btn::new("dxvk-apply", "Apply")
                            .compact()
                            .icon(Glyph::Save)
                            .grow()
                            .disabled(!can_act)
                            .on_click(self.dxvk_click(
                                cx,
                                "Apply DXVK settings",
                                "/api/dxvk-config",
                            )),
                    ),
            );
        let presets = div().flex().gap(px(6.)).children(
            [("Recommended", dxvk_recommended()), ("None", dxvk_none())]
                .into_iter()
                .enumerate()
                .map(|(index, (name, preset))| {
                    chip(
                        ("dxvk-preset", index),
                        name,
                        None,
                        self.dxvk_draft == preset,
                        !self.busy,
                        cx.listener(move |view, _: &ClickEvent, window, cx| {
                            view.dxvk_draft = preset.clone();
                            view.show_dxvk_draft(window, cx);
                            cx.notify();
                        }),
                    )
                }),
        );
        let threads = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .flex_1()
            .min_w_0()
            .child(div().w(px(80.)).child(self.field("dxvk-threads")))
            .child(chip(
                "threads-auto",
                "Recommended: Auto (0)",
                None,
                false,
                !self.busy,
                cx.listener(|view, _: &ClickEvent, window, cx| {
                    view.set_text("dxvk-threads", "0".into(), window, cx)
                }),
            ))
            .when_some(detected_threads, |this, threads| {
                this.child(chip(
                    "threads-manual",
                    format!("Manual: {threads}"),
                    None,
                    false,
                    !self.busy,
                    cx.listener(move |view, _: &ClickEvent, window, cx| {
                        view.set_text("dxvk-threads", threads.to_string(), window, cx)
                    }),
                ))
            });
        let tear_free = segmented(
            ["Auto", "True", "False"]
                .into_iter()
                .map(|mode| {
                    (
                        SharedString::from(mode),
                        self.dxvk_draft["tearFree"] == mode,
                        handler(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.dxvk_draft["tearFree"] = json!(mode);
                            cx.notify();
                        })),
                    )
                })
                .collect(),
            !self.busy,
        )
        .flex_1();
        let fps = self.slider_value("dxvk-fps", cx);
        bare_panel(
            "DXVK-GPLAsync",
            div()
                .flex()
                .flex_col()
                .gap(px(theme::PANEL_GAP))
                .child(control_bar)
                .children(self.progress_strip(&[Operation::DxvkInstall, Operation::DxvkUninstall]))
                .child(
                    columns()
                        .child(quick_row("Preset", presets))
                        .child(quick_row("Compiler threads", threads)),
                )
                .child(
                    columns()
                        .items_start()
                        .child(column(340.).child(group(
                            "Shader Pipeline",
                            vec![
                                self.dxvk_toggle(
                                    cx,
                                    "dxvk-async",
                                    "Async shader compilation",
                                    "enableAsync",
                                ),
                                self.dxvk_toggle(
                                    cx,
                                    "dxvk-gpl",
                                    "GPL async cache",
                                    "gplAsyncCache",
                                ),
                                self.dxvk_toggle(cx, "dxvk-hud", "FPS/GPU HUD", "hudEnabled"),
                            ],
                        )))
                        .child(column(340.).child(group(
                            "Frame Pacing",
                            vec![
                                    group_row(
                                        "Frame rate cap",
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(10.))
                                            .flex_1()
                                            .min_w(px(160.))
                                            .child(
                                                div().flex().flex_1().min_w(px(100.)).child(
                                                    Slider::new(&self.sliders["dxvk-fps"])
                                                        .disabled(self.busy),
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .w(px(36.))
                                                    .flex_none()
                                                    .text_right()
                                                    .font_weight(FontWeight::BOLD)
                                                    .child(fps.to_string()),
                                            ),
                                    ),
                                    group_row(
                                        "Frame latency",
                                        div().w(px(120.)).child(self.field("dxvk-latency")),
                                    ),
                                    group_row("Tear Free", tear_free),
                                ],
                        ))),
                ),
        )
    }

    pub(super) fn graphics_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.graphics_tab;
        let tabs = segmented(
            [
                (GraphicsTab::Settings, "Graphics Settings"),
                (GraphicsTab::Dxvk, "DXVK"),
            ]
            .into_iter()
            .map(|(target, label)| {
                (
                    SharedString::from(label),
                    tab == target,
                    handler(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.graphics_tab = target;
                        cx.notify();
                    })),
                )
            })
            .collect(),
            true,
        );
        div()
            .flex()
            .flex_col()
            .gap(px(theme::PANEL_GAP))
            .child(tabs)
            .child(match tab {
                GraphicsTab::Settings => self.graphics_settings(cx),
                GraphicsTab::Dxvk => self.dxvk_view(cx),
            })
            .into_any_element()
    }
}
