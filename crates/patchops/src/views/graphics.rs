//! Graphics tab (legacy `activeView === "graphics"`): sub-tabs "Graphics Settings" and "DXVK".
//!
//! * Graphics Settings: quick grid (preset / display mode / resolution / refresh rate), Performance
//!   and Quality groups and the Advanced drawer. Every control writes `config.ini` through
//!   `Action::SetConfig` & friends on its *commit* event only (Enter / blur / step for inputs,
//!   mouse-up / key press for sliders), as the legacy `NumberRow` / `TextRow` / `SliderControl`
//!   did. Commits equal to the saved value are skipped (legacy wrote them anyway on every blur).
//! * DXVK: a local draft (`DxvkSettings`) edited by the controls and applied with "Apply". Like the
//!   legacy effect on `[state.dxvk.settings]` the draft is re-seeded whenever a new state arrives.
//!
//! Stateful widgets are created once; `sync` pushes saved values into them (silently) whenever the
//! controller delivers a new state. `NumberInput` ignores pushes while typing and `Slider` while
//! dragging, and the resolution text only follows the saved value when that value changed, so
//! nothing the user is editing is clobbered.

use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyElement, Context, Entity, IntoElement, Render, Subscription, Window};
use patchops_core::ops::dxvk::DxvkSettings;
use patchops_core::State;

use crate::actions::{Action, ConfigId, ConfigInput};
use crate::controller::Controller;
use crate::theme::ActiveTheme;
use crate::views::{self, PANEL_GAP};
use crate::widgets::{
    Button, Chip, Collapsible, Group, Icon, NumberInput, NumberInputEvent, ProgressBar, ScrollArea, Select,
    SelectEvent, SettingRow, Slider, SliderEvent, StatusPill, SubTabs, TextInput, TextInputEvent, Toggle, Tone,
    tooltip,
};

const DISPLAY_MODES: [(&str, &str); 3] = [("0", "Windowed"), ("1", "Fullscreen"), ("2", "Fullscreen Windowed")];
const TEAR_FREE: [(&str, &str); 3] = [("Auto", "Auto"), ("True", "True"), ("False", "False")];
const DXVK_PRESETS: [&str; 2] = ["Recommended", "None"];

/// Legacy `dxvkPresets`.
fn dxvk_preset(name: &str) -> Option<DxvkSettings> {
    match name {
        "Recommended" => Some(DxvkSettings {
            enable_async: true,
            gpl_async_cache: true,
            num_compiler_threads: 0,
            max_frame_rate: 0,
            max_frame_latency: 1,
            tear_free: "True".into(),
            hud_enabled: false,
        }),
        "None" => Some(DxvkSettings {
            enable_async: true,
            gpl_async_cache: false,
            num_compiler_threads: 0,
            max_frame_rate: 0,
            max_frame_latency: 0,
            tear_free: "Auto".into(),
            hud_enabled: false,
        }),
        _ => None,
    }
}

/// Legacy `detectedCompilerThreads`: logical cores - 2, at least 1.
fn detected_threads() -> Option<i64> {
    std::thread::available_parallelism().ok().map(|n| (n.get() as i64 - 2).max(1))
}

/// Bordered tile row (legacy `.graphics-quick-grid .setting-row`): children laid out in a wrapping
/// row, so controls drop under their label instead of being squeezed when the tile is narrow.
fn tile_row(cx: &Context<GraphicsView>, children: impl IntoIterator<Item = AnyElement>) -> gpui::Div {
    let t = *cx.theme();
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(10.))
        .gap_y(px(6.))
        .min_h(px(TILE_MIN_H))
        .min_w_0()
        .px(px(14.))
        .py(px(5.))
        .border_1()
        .border_color(t.border)
        .rounded(px(t.radius - 4.))
        .bg(gpui::white().opacity(0.03))
        .text_size(px(t.font_md))
        .text_color(t.text_strong)
        .children(children)
}

/// Minimum width of a quick-settings tile (legacy `minmax(220px, 1fr)` plus tile padding).
const QUICK_TILE_MIN: f32 = 230.;
/// Minimum control width in a quick tile; the Preset select needs room for "Choose preset".
const QUICK_CTL_MIN: f32 = 96.;
const QUICK_PRESET_CTL_MIN: f32 = 150.;
/// Width of the DXVK "Frame latency" number field (legacy ~238px).
const FRAME_LATENCY_W: f32 = 238.;
/// Row heights: boxed setting tile, DXVK status pill / action strip.
const TILE_MIN_H: f32 = 50.;
const DXVK_BAR_H: f32 = 58.;

/// A column of the "groups" row: stretches to the row's height so both boxes share a bottom edge.
fn group_cell(min_w: f32, group: impl IntoElement) -> gpui::Div {
    views::fr(div(), 1.0).min_w(px(min_w)).flex().flex_col().child(group)
}

pub struct GraphicsView {
    c: Entity<Controller>,
    tab: usize,
    advanced_open: bool,
    // Graphics Settings
    preset: Entity<Select>,
    display: Entity<Select>,
    resolution: Entity<TextInput>,
    refresh: Entity<NumberInput>,
    max_fps: Entity<NumberInput>,
    render_res: Entity<Slider>,
    fov: Entity<Slider>,
    frame_latency: Entity<NumberInput>,
    vram: Entity<NumberInput>,
    // DXVK
    dx_preset: Entity<Select>,
    dx_threads: Entity<NumberInput>,
    dx_cap: Entity<Slider>,
    dx_latency: Entity<NumberInput>,
    dx_tear: Entity<Select>,
    draft: DxvkSettings,
    detected: Option<i64>,
    // sync bookkeeping
    last_state: Option<Arc<State>>,
    seeded_resolution: Option<String>,
    seeded_presets: Vec<String>,
    _subs: Vec<Subscription>,
}

impl GraphicsView {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        let mut subs = Vec::new();
        let num = |v: f64, lo: f64, hi: f64, cx: &mut Context<Self>| cx.new(|cx| NumberInput::new(v, lo, hi, cx));
        let preset = cx.new(|cx| Select::new(Vec::<(String, String)>::new(), cx).placeholder("Choose preset").momentary());
        let display = cx.new(|cx| Select::new(DISPLAY_MODES, cx));
        let resolution = cx.new(|cx| TextInput::new("1920x1080", cx));
        let refresh = num(60., 1., 240., cx);
        let max_fps = num(165., 0., 1000., cx);
        let render_res = cx.new(|cx| Slider::new(50., 200., 100., cx).step(1.).label_width(px(44.)));
        let fov = cx.new(|cx| Slider::new(65., 120., 80., cx).step(1.).label_width(px(44.)));
        let frame_latency = num(1., 0., 4., cx);
        let vram = num(75., 75., 100., cx);
        let dx_preset = cx.new(|cx| Select::new(DXVK_PRESETS.map(|p| (p, p)), cx).placeholder("Choose preset").momentary());
        let dx_threads = num(0., 0., 64., cx);
        let dx_cap = cx.new(|cx| Slider::new(0., 360., 0., cx).step(1.).label_width(px(44.)));
        let dx_latency = num(1., 0., 16., cx);
        let dx_tear = cx.new(|cx| Select::new(TEAR_FREE, cx));

        // ---- Graphics Settings: commit handlers ---------------------------------------------
        subs.push(cx.subscribe(&preset, |this, _, ev: &SelectEvent, cx| {
            let SelectEvent::Changed(name) = ev;
            this.run(Action::ApplyPreset(name.to_string()), cx);
        }));
        subs.push(cx.subscribe(&display, |this, _, ev: &SelectEvent, cx| {
            let SelectEvent::Changed(v) = ev;
            if let Ok(n) = v.parse::<i64>() {
                this.run(Action::SetConfig(ConfigId::DisplayMode, ConfigInput::Int(n)), cx);
            }
        }));
        subs.push(cx.subscribe(&resolution, |this, input, ev: &TextInputEvent, cx| {
            if let TextInputEvent::Submit(_) | TextInputEvent::Blurred(_) = ev {
                this.commit_resolution(&input, cx);
            }
        }));
        let int_num = |id: ConfigId| {
            move |this: &mut Self, _: Entity<NumberInput>, ev: &NumberInputEvent, cx: &mut Context<Self>| {
                if let NumberInputEvent::Committed(v) = ev {
                    this.commit_int(id, *v, cx);
                }
            }
        };
        subs.push(cx.subscribe(&refresh, int_num(ConfigId::RefreshRate)));
        subs.push(cx.subscribe(&max_fps, int_num(ConfigId::MaxFps)));
        subs.push(cx.subscribe(&frame_latency, int_num(ConfigId::MaxFrameLatency)));
        let int_slider = |id: ConfigId| {
            move |this: &mut Self, _: Entity<Slider>, ev: &SliderEvent, cx: &mut Context<Self>| {
                if let SliderEvent::Committed(v) = ev {
                    this.commit_int(id, f64::from(v.round()), cx);
                }
            }
        };
        subs.push(cx.subscribe(&render_res, int_slider(ConfigId::RenderResolution)));
        subs.push(cx.subscribe(&fov, int_slider(ConfigId::Fov)));
        subs.push(cx.subscribe(&vram, |this, _, ev: &NumberInputEvent, cx| {
            if let NumberInputEvent::Committed(v) = ev {
                let v = *v as i64;
                if this.state(cx).is_some_and(|s| s.advanced.vram_target != v) {
                    this.run(Action::VramTarget { limited: true, target: v }, cx);
                }
            }
        }));

        // ---- DXVK draft ----------------------------------------------------------------------
        subs.push(cx.subscribe(&dx_preset, |this, _, ev: &SelectEvent, cx| {
            let SelectEvent::Changed(name) = ev;
            if let Some(p) = dxvk_preset(name) {
                this.draft = p;
                this.push_draft(cx);
            }
        }));
        subs.push(cx.subscribe(&dx_threads, |this, _, ev: &NumberInputEvent, cx| {
            let (NumberInputEvent::Changed(v) | NumberInputEvent::Committed(v)) = ev;
            this.draft.num_compiler_threads = (*v as i64).clamp(0, 64);
            cx.notify();
        }));
        subs.push(cx.subscribe(&dx_cap, |this, _, ev: &SliderEvent, cx| {
            // Legacy commits the slider value into the draft on release; it is only a local draft,
            // so following the drag keeps the draft and the label consistent.
            let (SliderEvent::Changed(v) | SliderEvent::Committed(v)) = ev;
            this.draft.max_frame_rate = v.round() as i64;
            cx.notify();
        }));
        subs.push(cx.subscribe(&dx_latency, |this, _, ev: &NumberInputEvent, cx| {
            if let NumberInputEvent::Committed(v) = ev {
                this.draft.max_frame_latency = *v as i64;
                cx.notify();
            }
        }));
        subs.push(cx.subscribe(&dx_tear, |this, _, ev: &SelectEvent, cx| {
            let SelectEvent::Changed(v) = ev;
            this.draft.tear_free = v.to_string();
            cx.notify();
        }));

        subs.push(cx.observe(&controller, |this, _, cx| {
            this.sync(cx);
            cx.notify();
        }));

        let mut this = Self {
            c: controller,
            tab: 0,
            advanced_open: false,
            preset,
            display,
            resolution,
            refresh,
            max_fps,
            render_res,
            fov,
            frame_latency,
            vram,
            dx_preset,
            dx_threads,
            dx_cap,
            dx_latency,
            dx_tear,
            draft: dxvk_preset("Recommended").unwrap_or_default(),
            detected: detected_threads(),
            last_state: None,
            seeded_resolution: None,
            seeded_presets: Vec::new(),
            _subs: subs,
        };
        this.sync(cx);
        this
    }

    fn state(&self, cx: &Context<Self>) -> Option<Arc<State>> {
        self.c.read(cx).state().cloned()
    }

    fn run(&self, action: Action, cx: &mut Context<Self>) {
        self.c.update(cx, |c, cx| c.run(action, cx));
    }

    /// Run a numeric config write unless it equals the saved value.
    fn commit_int(&self, id: ConfigId, v: f64, cx: &mut Context<Self>) {
        let Some(s) = self.state(cx) else { return };
        let (g, a) = (&s.graphics, &s.advanced);
        let saved = match id {
            ConfigId::MaxFps => g.max_fps as f64,
            ConfigId::Fov => g.fov as f64,
            ConfigId::RefreshRate => g.refresh_rate,
            ConfigId::RenderResolution => g.render_resolution as f64,
            ConfigId::MaxFrameLatency => a.max_frame_latency as f64,
            _ => f64::NAN,
        };
        if (saved - v).abs() > 1e-9 || saved.is_nan() {
            let input = if v.fract() == 0.0 { ConfigInput::Int(v as i64) } else { ConfigInput::Float(v) };
            self.run(Action::SetConfig(id, input), cx);
        }
    }

    /// Legacy `TextRow` commit (Enter / blur). Legacy accepts any text ("any text"); an empty
    /// value would only blank `WindowSize`, so it snaps back to the saved value instead.
    fn commit_resolution(&self, input: &Entity<TextInput>, cx: &mut Context<Self>) {
        let Some(s) = self.state(cx) else { return };
        let text = input.read(cx).text().trim().to_string();
        if text.is_empty() {
            let saved = s.graphics.resolution.clone();
            input.update(cx, |i, cx| i.set_text(saved, cx));
        } else if text != s.graphics.resolution {
            self.run(Action::SetConfig(ConfigId::Resolution, ConfigInput::Text(text)), cx);
        }
    }

    /// The DXVK draft as of *now*: number inputs that are still being edited are committed first
    /// (a same-frame click on Install / Apply can arrive before the blur commit reached the draft,
    /// and the closure must not use a copy captured at render time).
    fn live_draft(&mut self, cx: &mut Context<Self>) -> DxvkSettings {
        let threads = self.dx_threads.update(cx, |n, cx| n.commit_if_editing(cx));
        let latency = self.dx_latency.update(cx, |n, cx| n.commit_if_editing(cx));
        self.draft.num_compiler_threads = (threads as i64).clamp(0, 64);
        self.draft.max_frame_latency = latency as i64;
        self.draft.clone()
    }

    /// Push the DXVK draft into the DXVK widgets (no events).
    fn push_draft(&mut self, cx: &mut Context<Self>) {
        let d = self.draft.clone();
        self.dx_threads.update(cx, |n, cx| n.set_value(d.num_compiler_threads as f64, cx));
        self.dx_cap.update(cx, |s, cx| {
            s.set_value_silent(d.max_frame_rate as f32);
            cx.notify();
        });
        self.dx_latency.update(cx, |n, cx| n.set_value(d.max_frame_latency as f64, cx));
        self.dx_tear.update(cx, |s, cx| s.set_selected(Some(&d.tear_free), cx));
        cx.notify();
    }

    /// Mirror a freshly delivered state into the widgets (legacy `useEffect([value])` per row).
    fn sync(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.state(cx) else { return };
        if self.last_state.as_ref().is_some_and(|l| Arc::ptr_eq(l, &s)) {
            return;
        }
        self.last_state = Some(s.clone());
        let (g, a) = (&s.graphics, &s.advanced);

        if self.seeded_presets != s.presets {
            self.seeded_presets = s.presets.clone();
            self.preset.update(cx, |p, cx| p.set_options(s.presets.iter().map(|n| (n.clone(), n.clone())), cx));
        }
        self.display.update(cx, |d, cx| d.set_selected(Some(&g.display_mode.to_string()), cx));
        if self.seeded_resolution.as_deref() != Some(g.resolution.as_str()) {
            self.seeded_resolution = Some(g.resolution.clone());
            self.resolution.update(cx, |i, cx| i.set_text(g.resolution.clone(), cx));
        }
        self.refresh.update(cx, |n, cx| n.set_value(g.refresh_rate, cx));
        self.max_fps.update(cx, |n, cx| n.set_value(g.max_fps as f64, cx));
        for (slider, v) in [(&self.render_res, g.render_resolution), (&self.fov, g.fov)] {
            slider.update(cx, |s, cx| {
                s.set_value_silent(v as f32);
                cx.notify();
            });
        }
        self.frame_latency.update(cx, |n, cx| n.set_value(a.max_frame_latency as f64, cx));
        self.vram.update(cx, |n, cx| {
            n.set_value(a.vram_target as f64, cx);
            n.set_disabled(!a.vram_limited, cx);
        });

        self.draft = s.dxvk.settings.clone();
        self.push_draft(cx);
    }

    fn graphics_tab(&self, window: &Window, cx: &mut Context<Self>, state: &State) -> impl IntoElement {
        let c = self.c.clone();
        let this = cx.entity();
        let (g, a) = (&state.graphics, &state.advanced);
        // Legacy `.graphics-quick-grid`: auto-fit columns of at least 220px; each tile is a bordered
        // row with a label column (min 88px) and a control that fills the rest.
        let tile = |label: &'static str, ctl_min: f32, ctl: AnyElement| {
            tile_row(cx, [
                div().flex_none().whitespace_nowrap().child(label).into_any_element(),
                views::fr(div(), 1.0).min_w(px(ctl_min)).flex().flex_col().child(ctl).into_any_element(),
            ])
        };
        // Legacy `repeat(auto-fit, minmax(220px, 1fr))`: a real grid, so an orphan tile keeps the
        // width of one column instead of stretching across the row.
        let cols = (((views::content_width(window) + PANEL_GAP) / (QUICK_TILE_MIN + PANEL_GAP)).floor() as u16).clamp(1, 4);
        let quick = div()
            .grid()
            .grid_cols(cols)
            .gap(px(PANEL_GAP))
            .child(tile("Preset", QUICK_PRESET_CTL_MIN, self.preset.clone().into_any_element()))
            .child(tile("Display Mode", QUICK_CTL_MIN, self.display.clone().into_any_element()))
            .child(tile("Resolution", QUICK_CTL_MIN, self.resolution.clone().into_any_element()))
            .child(tile("Refresh Rate", QUICK_CTL_MIN, self.refresh.clone().into_any_element()));

        let cfg_toggle = |id: &'static str, on: bool, cid: ConfigId| {
            let c = c.clone();
            Toggle::new(id, on).on_change(move |v, _, cx| {
                c.update(cx, |c, cx| c.run(Action::SetConfig(cid, ConfigInput::Bool(v)), cx))
            })
        };
        // Number / slider rows: the control fills its column. Toggle rows: the switch sits next to
        // the label (legacy `ToggleRow`).
        let row = |label: &'static str, ctl: gpui::AnyElement, last: bool| {
            let r = SettingRow::new(label).control(ctl).control_start();
            if last { r.last() } else { r }
        };
        let fill_row = |label: &'static str, ratio: f32, ctl: gpui::AnyElement, last: bool| {
            let r = SettingRow::new(label).control(ctl).fill(ratio);
            if last { r.last() } else { r }
        };

        let performance = Group::new("Performance")
            .child(fill_row("Max FPS", 0.45, self.max_fps.clone().into_any_element(), false))
            .child(row("Vertical sync", cfg_toggle("vsync", g.vsync, ConfigId::Vsync).into_any_element(), false))
            .child(row("FPS counter", cfg_toggle("drawfps", g.draw_fps, ConfigId::DrawFps).into_any_element(), true));
        let quality = Group::new("Quality")
            .child(fill_row("Render Resolution", 0.34, self.render_res.clone().into_any_element(), false))
            .child(fill_row("Field of View", 0.34, self.fov.clone().into_any_element(), true));
        // Legacy `.graphics-settings-shell`: rows `auto 1fr auto`, so the groups take the free height
        // while the drawer is closed (open: natural heights, the page scrolls).
        let grow = !self.advanced_open;
        let groups = div()
            .flex()
            .flex_wrap()
            .gap(px(PANEL_GAP))
            .when(grow, |d| d.flex_grow().flex_shrink_0())
            .child(group_cell(340., performance))
            .child(group_cell(340., quality));

        let c2 = c.clone();
        let c3 = c.clone();
        let target = a.vram_target;
        let advanced = Collapsible::new("gfx-advanced", "Advanced", self.advanced_open)
            .detail("Config, CPU, latency, and VRAM controls")
            .chevron_end()
            .on_toggle(move |open, _, cx| this.update(cx, |v, cx| {
                v.advanced_open = open;
                cx.notify();
            }))
            .child(row("Smooth framerate", cfg_toggle("smooth", a.smooth_framerate, ConfigId::SmoothFramerate).into_any_element(), false))
            .child(row("Expose hidden graphics", cfg_toggle("unlock", a.unlock_options, ConfigId::UnlockOptions).into_any_element(), false))
            .child(row("Reduce CPU pressure", cfg_toggle("cpu", a.reduce_cpu, ConfigId::ReduceCpu).into_any_element(), false))
            .child(fill_row("Frame latency", 0.45, self.frame_latency.clone().into_any_element(), false))
            .child(row(
                "Limit VRAM target",
                Toggle::new("vram-limit", a.vram_limited)
                    .on_change(move |v, _, cx| {
                        c2.update(cx, |c, cx| c.run(Action::VramTarget { limited: v, target }, cx))
                    })
                    .into_any_element(),
                false,
            ))
            .child(fill_row("VRAM target %", 0.45, self.vram.clone().into_any_element(), false))
            .child(row(
                "Lock config.ini",
                Toggle::new("readonly", a.config_readonly)
                    .on_change(move |v, _, cx| c3.update(cx, |c, cx| c.run(Action::ConfigReadonly(v), cx)))
                    .into_any_element(),
                true,
            ));

        div()
            .flex()
            .flex_col()
            .gap(px(PANEL_GAP))
            .when(grow, |d| d.flex_grow().flex_shrink_0())
            .child(quick)
            .child(groups)
            .child(advanced)
    }

    fn dxvk_tab(&self, cx: &mut Context<Self>, state: &State) -> impl IntoElement {
        let c = self.c.clone();
        let ctl = self.c.read(cx);
        let installing = ctl.is_busy("dxvk-install");
        let progress = ctl.progress("dxvk-install");
        let (b_uninstall, b_apply) = (ctl.is_busy("dxvk-uninstall"), ctl.is_busy("dxvk-config"));
        let this = cx.entity();
        let t = *cx.theme();
        let d = &self.draft;
        let dx = &state.dxvk;

        let pill = |label: &'static str, value: &'static str, ok: bool| {
            views::fr(div(), 1.0).min_w(px(250.)).child(
                StatusPill::new(label, value, Tone::from_ok(ok)).inline(true).min_h(DXVK_BAR_H),
            )
        };
        let actions = views::fr(div(), 1.0)
            .min_w(px(360.))
            .min_h(px(DXVK_BAR_H))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .p(px(10.))
            .border_1()
            .border_color(t.border)
            .rounded(px(t.radius - 4.))
            .bg(gpui::white().opacity(0.03))
            .child(div().flex_1().min_w(px(90.)).child(
                Button::new("dxvk-install", "Install")
                    .icon(Icon::Download)
                    .small()
                    .full_width()
                    .busy(installing)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let d = this.live_draft(cx);
                        this.run(Action::DxvkInstall(d), cx);
                    })),
            ))
            .child(div().flex_1().min_w(px(110.)).child(
                Button::new("dxvk-uninstall", "Uninstall")
                    .icon(Icon::Trash2)
                    .small()
                    .full_width()
                    .disabled(!dx.installed)
                    .busy(b_uninstall)
                    .on_click(views::run_confirmed(&c, Action::DxvkUninstall)),
            ))
            .child(div().flex_1().min_w(px(90.)).child(
                Button::new("dxvk-apply", "Apply")
                    .icon(Icon::Save)
                    .small()
                    .full_width()
                    .busy(b_apply)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let d = this.live_draft(cx);
                        this.run(Action::DxvkConfig(d), cx);
                    })),
            ));
        let bar = div()
            .flex()
            .flex_wrap()
            .gap(px(PANEL_GAP))
            .child(pill("Status", if dx.installed { "Installed" } else { "Not installed" }, dx.installed))
            .child(pill("dxvk.conf", if dx.conf_exists { "Configured" } else { "Missing" }, dx.conf_exists))
            .child(actions);

        let t1 = this.clone();
        let t2 = this.clone();
        let threads = d.num_compiler_threads;
        let detected = self.detected;
        let cores = std::thread::available_parallelism().map(|n| n.get()).ok();
        let auto_tip = match (detected, cores) {
            (Some(n), Some(c)) => format!("Auto uses about {n} compiler threads on this {c}-thread CPU"),
            _ => "Auto lets DXVK choose compiler threads".to_string(),
        };
        let threads_row = tile_row(
            cx,
            [
                div().flex_none().child("Compiler threads").into_any_element(),
                div()
                    .flex()
                    .flex_wrap()
                    .flex_1()
                    .min_w_0()
                    .justify_start()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(90.)).child(self.dx_threads.clone()))
                    .child(
                        div().id("dx-auto-tip").tooltip(tooltip(auto_tip)).child(
                            Chip::new("dx-auto", "Recommended: Auto (0)").height(30.).active(threads == 0).on_click(move |_, _, cx| {
                                t1.update(cx, |v, cx| {
                                    v.draft.num_compiler_threads = 0;
                                    v.push_draft(cx);
                                })
                            }),
                        ),
                    )
                    .children(detected.map(|n| {
                        let tip = match cores {
                            Some(c) => format!("{c} logical CPU cores detected"),
                            None => "Logical CPU cores detected".to_string(),
                        };
                        div().id("dx-manual-tip").tooltip(tooltip(tip)).child(
                            Chip::new("dx-manual", format!("Manual: {n}")).height(30.).dim(true).active(threads == n && n != 0).on_click(
                                move |_, _, cx| {
                                    t2.update(cx, |v, cx| {
                                        v.draft.num_compiler_threads = n;
                                        v.push_draft(cx);
                                    })
                                },
                            ),
                        )
                    }))
                    .into_any_element(),
            ],
        );
        // Legacy `.dxvk-quick-grid` is the shell's `1fr` row: the tiles take the free height.
        let quick = div()
            .flex()
            .flex_wrap()
            .gap(px(PANEL_GAP))
            .flex_grow()
            .flex_shrink_0()
            .child(views::fr(div(), 1.0).min_w(px(280.)).flex().flex_col().child(
                SettingRow::new("Preset").control(self.dx_preset.clone()).fill(0.5).boxed(),
            ))
            .child(views::fr(div(), 1.0).min_w(px(400.)).flex().flex_col().child(threads_row.flex_grow()));

        let toggle = |id: &'static str, on: bool, set: fn(&mut DxvkSettings, bool)| {
            let this = this.clone();
            Toggle::new(id, on).on_change(move |v, _, cx| {
                this.update(cx, |view, cx| {
                    set(&mut view.draft, v);
                    cx.notify();
                })
            })
        };
        let row = |label: &'static str, ctl: gpui::AnyElement, last: bool| {
            let r = SettingRow::new(label).control(ctl).control_start();
            if last { r.last() } else { r }
        };
        let fill_row = |label: &'static str, ratio: f32, ctl: gpui::AnyElement, last: bool| {
            let r = SettingRow::new(label).control(ctl).fill(ratio);
            if last { r.last() } else { r }
        };
        let shader = Group::new("Shader Pipeline")
            .child(row("Async shader compilation", toggle("dx-async", d.enable_async, |s, v| s.enable_async = v).into_any_element(), false))
            .child(row("GPL async cache", toggle("dx-gpl", d.gpl_async_cache, |s, v| s.gpl_async_cache = v).into_any_element(), false))
            .child(row("FPS/GPU HUD", toggle("dx-hud", d.hud_enabled, |s, v| s.hud_enabled = v).into_any_element(), true));
        let pacing = Group::new("Frame Pacing")
            .child(fill_row("Frame rate cap", 0.34, self.dx_cap.clone().into_any_element(), false))
            .child(fill_row("Frame latency", 0.45, div().flex().flex_col().flex_none().w(px(FRAME_LATENCY_W)).max_w_full().child(self.dx_latency.clone()).into_any_element(), false))
            .child(fill_row("Tear Free", 0.45, self.dx_tear.clone().into_any_element(), true));
        let groups = div()
            .flex()
            .flex_wrap()
            .gap(px(PANEL_GAP))
            .child(group_cell(340., shader))
            .child(group_cell(340., pacing));

        div()
            .flex()
            .flex_col()
            .gap(px(PANEL_GAP))
            .flex_grow()
            .flex_shrink_0()
            .child(bar)
            .when(installing, |d| {
                d.child(ProgressBar::new("dxvk-progress").value(progress.map(|p| f32::from(p) / 100.)))
            })
            .child(quick)
            .child(groups)
    }
}

impl Render for GraphicsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.state(cx) else { return div().into_any_element() };
        let this = cx.entity();
        let heading = if self.tab == 0 { "Graphics Settings" } else { "DXVK-GPLAsync" };
        let body = if self.tab == 0 {
            self.graphics_tab(window, cx, &state).into_any_element()
        } else {
            self.dxvk_tab(cx, &state).into_any_element()
        };
        div().size_full().flex().flex_col().min_h_0().child(ScrollArea::new(("graphics-scroll", self.tab))
            .gap(px(PANEL_GAP))
            .child(div().flex_none().child(SubTabs::new("graphics-tabs", ["Graphics Settings", "DXVK"]).full_width().selected(self.tab).on_change(
                move |ix, _, cx| {
                    this.update(cx, |v, cx| {
                        v.tab = ix;
                        cx.notify();
                    })
                },
            )))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_grow()
                    .flex_shrink_0()
                    .child(div().flex_none().child(views::heading(cx, heading)))
                    .child(body),
            ))
            .into_any_element()
    }
}
