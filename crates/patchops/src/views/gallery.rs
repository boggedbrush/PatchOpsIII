//! Widget gallery: every widget in every state. Reachable with `PATCHOPS_GALLERY=1`
//! (adds a "Gallery" sidebar entry and opens it first). Used for visual regression by eye.

use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, FontWeight, Render, SharedString, Window,
};

use crate::theme::ActiveTheme;
use crate::widgets::*;

pub fn enabled() -> bool {
    std::env::var_os("PATCHOPS_GALLERY").is_some_and(|v| !v.is_empty() && v != "0")
}

const TABS: [&str; 4] = ["Controls", "Inputs", "Display", "Overlays"];

pub struct GalleryView {
    tab: usize,
    toggle: bool,
    check: bool,
    radio: usize,
    seg: usize,
    sub: usize,
    nav: usize,
    collapsed: bool,
    show_pw: bool,
    busy: bool,
    chip: usize,
    // overlays
    dialog: bool,
    confirm: bool,
    browser_open: bool,
    // entities
    input: Entity<TextInput>,
    input_disabled: Entity<TextInput>,
    input_ro: Entity<TextInput>,
    input_pw: Entity<TextInput>,
    input_max: Entity<TextInput>,
    number: Entity<NumberInput>,
    number_dis: Entity<NumberInput>,
    slider: Entity<Slider>,
    slider_dis: Entity<Slider>,
    select: Entity<Select>,
    select_preset: Entity<Select>,
    select_dis: Entity<Select>,
    log: Entity<LogList>,
    toasts: Entity<ToastHost>,
    browser: Entity<DirBrowser>,
    last_event: SharedString,
}

fn fake_listing(path: &str) -> BrowseData {
    let base = if path.is_empty() {
        "/home/user/Games".to_string()
    } else {
        path.to_string()
    };
    let ent = |n: &str, exe: bool| BrowseEntry {
        name: n.into(),
        path: format!("{}/{}", base.trim_end_matches('/'), n),
        has_game_executable: exe,
    };
    BrowseData {
        path: base.clone(),
        parent: Some("/home/user".into()),
        has_game_executable: base.ends_with("Black Ops III"),
        roots: vec![BrowseLocation {
            label: "/".into(),
            path: "/".into(),
        }],
        shortcuts: vec![
            BrowseLocation {
                label: "Home".into(),
                path: "/home/user".into(),
            },
            BrowseLocation {
                label: "Steam Library".into(),
                path: "/home/user/.steam/steamapps/common".into(),
            },
            BrowseLocation {
                label: "Documents".into(),
                path: "/home/user/Documents".into(),
            },
        ],
        entries: [
            "Call of Duty Black Ops III",
            "Black Ops III",
            "Proton",
            "Steam",
            "Wine Prefixes",
            "mods",
        ]
        .iter()
        .map(|n| ent(n, *n == "Black Ops III"))
        .chain((0..30).map(|i| ent(&format!("folder-{i:02}"), false)))
        .collect(),
    }
}

impl GalleryView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Black Ops III directory", cx));
        let input_disabled = cx.new(|cx| {
            let mut i = TextInput::new("Disabled", cx);
            i.set_text("t7patch.conf missing", cx);
            i.set_disabled(true, cx);
            i
        });
        let input_ro = cx.new(|cx| {
            let mut i = TextInput::new("", cx);
            i.set_text(
                "/home/user/.steam/steamapps/common/Call of Duty Black Ops III",
                cx,
            );
            i.set_read_only(true, cx);
            i
        });
        let input_pw = cx.new(|cx| {
            let mut i = TextInput::new("Enter network password", cx);
            i.set_masked(true, cx);
            i.set_text("hunter2", cx);
            i
        });
        let input_max = cx.new(|cx| {
            let mut i = TextInput::new("Gamertag (max 20)", cx);
            i.set_max_len(Some(20));
            i
        });
        let number = cx.new(|cx| NumberInput::new(165., 1., 240., cx));
        let number_dis = cx.new(|cx| {
            let mut n = NumberInput::new(100., 75., 100., cx);
            n.set_disabled(true, cx);
            n
        });
        let slider = cx.new(|cx| Slider::new(65., 120., 80., cx));
        let slider_dis = cx.new(|cx| {
            let mut s = Slider::new(50., 200., 100., cx);
            s.set_disabled(true, cx);
            s
        });
        let select = cx.new(|cx| {
            let mut s = Select::new(
                [
                    ("0", "Windowed"),
                    ("1", "Fullscreen"),
                    ("2", "Fullscreen Windowed"),
                ],
                cx,
            );
            s.set_selected(Some("1"), cx);
            s
        });
        let select_preset = cx.new(|cx| {
            Select::new(
                (0..14).map(|i| (format!("p{i}"), format!("Preset number {i}"))),
                cx,
            )
            .placeholder("Choose preset")
            .momentary()
        });
        let select_dis = cx.new(|cx| {
            let mut s = Select::new([("a", "Auto")], cx);
            s.set_selected(Some("a"), cx);
            s.set_disabled(true, cx);
            s
        });
        let log = cx.new(|cx| {
            let mut l = LogList::new(cx);
            l.push(LogLevel::Info, "PatchOpsIII started", cx);
            l.push(LogLevel::Success, "Game directory detected", cx);
            l.push(
                LogLevel::Warning,
                "t7patch.conf not found, using defaults",
                cx,
            );
            l.push(
                LogLevel::Error,
                "Failed to download DXVK: connection reset",
                cx,
            );
            for i in 0..40 {
                l.push(LogLevel::Info, format!("Copied file {i} of 40"), cx);
            }
            l
        });
        let toasts = cx.new(|_| ToastHost::new());
        let browser = cx.new(|cx| {
            DirBrowser::new(
                "Select Game Directory",
                "Choose the folder that contains BlackOps3.exe or BlackOpsIII.exe.",
                "Use Selected Folder",
                cx,
            )
            .require_game_executable(true)
        });
        cx.subscribe(&browser, |this, b, ev: &DirBrowserEvent, cx| match ev {
            DirBrowserEvent::Navigate(p) => {
                let d = fake_listing(p);
                b.update(cx, |b, cx| b.set_data(d, cx));
            }
            DirBrowserEvent::Select(p) => {
                this.browser_open = false;
                this.last_event = format!("selected {p}").into();
                cx.notify();
            }
            DirBrowserEvent::Cancel => {
                this.browser_open = false;
                cx.notify();
            }
        })
        .detach();
        cx.subscribe(&slider, |this, _, ev: &SliderEvent, cx| {
            if let SliderEvent::Committed(v) = ev {
                this.last_event = format!("slider committed {v}").into();
                cx.notify();
            }
        })
        .detach();
        cx.subscribe(&select, |this, _, SelectEvent::Changed(v), cx| {
            this.last_event = format!("select -> {v}").into();
            cx.notify();
        })
        .detach();
        cx.subscribe(&number, |this, _, ev: &NumberInputEvent, cx| {
            if let NumberInputEvent::Committed(v) = ev {
                this.last_event = format!("number committed {v}").into();
                cx.notify();
            }
        })
        .detach();
        if std::env::var_os("PATCHOPS_GALLERY_BROWSER").is_some() {
            browser.update(cx, |b, cx| b.navigate("", cx));
        }
        let tab = std::env::var("PATCHOPS_GALLERY_TAB")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Self {
            tab,
            toggle: true,
            check: true,
            radio: 0,
            seg: 0,
            sub: 0,
            nav: 1,
            collapsed: false,
            show_pw: false,
            busy: true,
            chip: 1,
            dialog: std::env::var_os("PATCHOPS_GALLERY_DIALOG").is_some(),
            confirm: false,
            browser_open: std::env::var_os("PATCHOPS_GALLERY_BROWSER").is_some(),
            input,
            input_disabled,
            input_ro,
            input_pw,
            input_max,
            number,
            number_dis,
            slider,
            slider_dis,
            select,
            select_preset,
            select_dis,
            log,
            toasts,
            browser,
            last_event: "".into(),
        }
    }

    fn section(&self, title: &str, cx: &Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        div().flex().flex_col().gap(px(8.)).child(
            div()
                .text_size(px(t.font_lg))
                .font_weight(FontWeight::BOLD)
                .child(title.to_string()),
        )
    }

    fn controls(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        let e = cx.entity();
        let tog = {
            let e = e.clone();
            move |v: bool, _: &mut Window, cx: &mut gpui::App| {
                e.update(cx, |g, cx| {
                    g.toggle = v;
                    cx.notify()
                })
            }
        };
        let chk = {
            let e = e.clone();
            move |v: bool, _: &mut Window, cx: &mut gpui::App| {
                e.update(cx, |g, cx| {
                    g.check = v;
                    cx.notify()
                })
            }
        };
        let row = || div().flex().flex_wrap().items_center().gap(px(10.));
        let radios = (0..3).map(|i| {
            let e = e.clone();
            Radio::new(("radio", i), self.radio == i)
                .label(
                    [
                        "Default (None)",
                        "Play Offline",
                        "All-around Enhancement Lite",
                    ][i],
                )
                .on_select(move |_, cx| {
                    e.update(cx, |g, cx| {
                        g.radio = i;
                        cx.notify()
                    })
                })
        });
        let seg = {
            let e = e.clone();
            move |i, _: &mut Window, cx: &mut gpui::App| {
                e.update(cx, |g, cx| {
                    g.seg = i;
                    cx.notify()
                })
            }
        };
        let sub = {
            let e = e.clone();
            move |i, _: &mut Window, cx: &mut gpui::App| {
                e.update(cx, |g, cx| {
                    g.sub = i;
                    cx.notify()
                })
            }
        };
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                self.section("Buttons", cx)
                    .child(
                        row()
                            .child(
                                Button::new("b1", "Launch Game")
                                    .primary()
                                    .icon(Icon::SquarePlay),
                            )
                            .child(Button::new("b2", "Browse...").icon(Icon::FolderOpen))
                            .child(Button::new("b3", "Uninstall").danger().icon(Icon::Trash2))
                            .child(Button::new("b4", "Ghost").ghost())
                            .child(Button::new("b5", "Disabled").disabled(true))
                            .child(
                                Button::new("b6", "Installing")
                                    .primary()
                                    .busy(self.busy)
                                    .icon(Icon::Download),
                            )
                            .child(Button::new("b7", "Small").small().icon(Icon::Save)),
                    )
                    .child(
                        row()
                            .child(Button::new("b8", "Busy secondary").busy(true))
                            .child(
                                Button::new("b9", "Disabled primary")
                                    .primary()
                                    .disabled(true)
                                    .icon(Icon::Download),
                            )
                            .child(
                                Button::new("b10", "Toggle busy")
                                    .small()
                                    .on_click(cx.listener(|g, _, _, cx| {
                                        g.busy = !g.busy;
                                        cx.notify()
                                    })),
                            )
                            .child(IconButton::new("ib1", Icon::X))
                            .child(
                                IconButton::new("ib2", Icon::KeyRound)
                                    .active(self.show_pw)
                                    .on_click(cx.listener(|g, _, _, cx| {
                                        g.show_pw = !g.show_pw;
                                        cx.notify()
                                    })),
                            )
                            .child(IconButton::new("ib3", Icon::Eye).disabled(true)),
                    ),
            )
            .child(
                self.section("Toggle / Checkbox / Radio", cx)
                    .child(
                        row()
                            .child(Toggle::new("t1", self.toggle).on_change(tog))
                            .child(Toggle::new("t2", false))
                            .child(Toggle::new("t3", true).disabled(true))
                            .child(
                                Checkbox::new("c1", self.check)
                                    .label("Use latest d3dcompiler")
                                    .on_change(chk),
                            )
                            .child(Checkbox::new("c2", false).label("Skip Intro"))
                            .child(Checkbox::new("c3", true).label("Disabled").disabled(true)),
                    )
                    .child(row().gap(px(24.)).children(radios)),
            )
            .child(
                self.section("Segmented / Tabs / Nav", cx)
                    .child(
                        div().w(px(260.)).child(
                            Segmented::new("seg", ["Stable", "Beta"])
                                .selected(self.seg)
                                .on_change(seg),
                        ),
                    )
                    .child(
                        SubTabs::new("sub", ["Graphics Settings", "DXVK"])
                            .selected(self.sub)
                            .on_change(sub),
                    )
                    .child(
                        div()
                            .w(px(200.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .p(px(10.))
                            .rounded(px(t.radius))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.sidebar)
                            .children(
                                [
                                    (Icon::LayoutDashboard, "Dashboard"),
                                    (Icon::ShieldCheck, "T7 Patch"),
                                    (Icon::RefreshCw, "EXE Swapper"),
                                    (Icon::Gem, "Enhanced"),
                                ]
                                .into_iter()
                                .enumerate()
                                .map(|(i, (ic, l))| {
                                    let e = e.clone();
                                    NavItem::new(("nav", i), ic, l)
                                        .active(self.nav == i)
                                        .on_click(move |_, _, cx| {
                                            e.update(cx, |g, cx| {
                                                g.nav = i;
                                                cx.notify()
                                            })
                                        })
                                }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn inputs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        let pw = self.show_pw;
        self.input_pw.update(cx, |i, cx| i.set_masked(!pw, cx));
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                self.section("Text input", cx)
                    .child(
                        FormRow::new("Game Directory")
                            .help("Editable with placeholder")
                            .control(self.input.clone()),
                    )
                    .child(FormRow::new("Disabled").control(self.input_disabled.clone()))
                    .child(FormRow::new("Read-only").control(self.input_ro.clone()))
                    .child(
                        FormRow::new("Password")
                            .help("Masked; the key button reveals it")
                            .control(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(div().flex_1().child(self.input_pw.clone()))
                                    .child(
                                        IconButton::new("pw-eye", Icon::KeyRound)
                                            .active(self.show_pw)
                                            .on_click(cx.listener(|g, _, _, cx| {
                                                g.show_pw = !g.show_pw;
                                                cx.notify()
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        FormRow::new("Gamertag")
                            .help("Max 20 characters")
                            .control(self.input_max.clone())
                            .last(),
                    ),
            )
            .child(
                self.section("Number / Slider / Select", cx)
                    .child(FormRow::new("Refresh Rate").control(self.number.clone()))
                    .child(FormRow::new("VRAM target %").control(self.number_dis.clone()))
                    .child(FormRow::new("Field of View").control(self.slider.clone()))
                    .child(FormRow::new("Render Resolution").control(self.slider_dis.clone()))
                    .child(FormRow::new("Display Mode").control(self.select.clone()))
                    .child(FormRow::new("Preset").control(self.select_preset.clone()))
                    .child(
                        FormRow::new("Tear Free")
                            .control(self.select_dis.clone())
                            .last(),
                    ),
            )
            .child(
                div()
                    .text_color(t.text_muted)
                    .text_size(px(t.font_xs))
                    .child(format!("last event: {}", self.last_event)),
            )
            .into_any_element()
    }

    fn display(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        let e = cx.entity();
        let colors: [(&str, u32); 4] = [
            ("Default", 0xf6f6f6),
            ("Red", 0xff453a),
            ("Green", 0x34c759),
            ("Blue", 0x5ac8fa),
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .gap(px(14.))
                    .child(
                        div().flex_1().child(
                            Panel::new("Status Overview")
                                .child(StatusRow::new("T7 Patch", Tone::Ok).value("Configured"))
                                .child(
                                    StatusRow::new("DXVK-GPLAsync", Tone::Neutral)
                                        .value("Not configured"),
                                )
                                .child(StatusRow::new("Launch Option", Tone::Warn).value("Offline"))
                                .child(
                                    StatusRow::new("Quality of Life", Tone::Neutral)
                                        .value("Not configured")
                                        .last(),
                                ),
                        ),
                    )
                    .child(
                        div().flex_1().child(
                            Panel::new("Launch Options")
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .h(px(32.))
                                        .child("Reforged")
                                        .child(
                                            div()
                                                .text_color(t.ok)
                                                .font_weight(FontWeight::BOLD)
                                                .child("Installed"),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .h(px(32.))
                                        .child("Ultimate Experience")
                                        .child(
                                            div()
                                                .text_color(t.warning)
                                                .font_weight(FontWeight::BOLD)
                                                .child("Not subscribed"),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(8.))
                                        .pt(px(8.))
                                        .child(Button::new("p1", "Install").small().full_width())
                                        .child(Button::new("p2", "Refresh").small().full_width())
                                        .child(Button::new("p3", "Apply").small().full_width()),
                                ),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(div().flex_1().child(StatusPill::new(
                        "Config",
                        "t7patch.conf found",
                        Tone::Ok,
                    )))
                    .child(div().flex_1().child(StatusPill::new(
                        "Game Mode",
                        "Unknown",
                        Tone::Neutral,
                    )))
                    .child(div().flex_1().child(StatusPill::new(
                        "Status",
                        "Not installed",
                        Tone::Warn,
                    ))),
            )
            .child(
                div()
                    .flex()
                    .gap(px(14.))
                    .child(
                        div().flex_1().child(
                            Card::new()
                                .icon(Icon::Cpu)
                                .title("System")
                                .child(Metric::new("Platform", "linux", Tone::Ok))
                                .child(Metric::new("Steam ID", "Not found", Tone::Neutral))
                                .child(Metric::new("PatchOps", "1.3.2", Tone::Ok)),
                        ),
                    )
                    .child(
                        div().flex_1().child(
                            Group::new("Performance")
                                .child(
                                    SettingRow::new("Vertical sync")
                                        .control(Toggle::new("g1", true)),
                                )
                                .child(
                                    SettingRow::new("FPS counter")
                                        .help("Draws the in-game counter")
                                        .control(Toggle::new("g2", false))
                                        .last(),
                                ),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(10.))
                    .child(Badge::new("Best for mods"))
                    .child(Badge::new("Active").tone(Tone::Ok))
                    .child(Badge::new("Not installed").tone(Tone::Warn))
                    .child(Badge::new("Failed").tone(Tone::Danger))
                    .children(colors.iter().enumerate().map(|(i, (l, c))| {
                        let e = e.clone();
                        Chip::new(("chip", i), *l)
                            .swatch(gpui::rgb(*c).into())
                            .active(self.chip == i)
                            .on_click(move |_, _, cx| {
                                e.update(cx, |g, cx| {
                                    g.chip = i;
                                    cx.notify()
                                })
                            })
                    }))
                    .child(Spinner::new("sp").size(px(20.)))
                    .child(div().w(px(120.)).child(ProgressBar::new("pb1")))
                    .child(
                        div()
                            .w(px(120.))
                            .child(ProgressBar::new("pb2").value(Some(0.6))),
                    ),
            )
            .child(
                Alert::error("Could not reach the PatchOpsIII service.").on_dismiss(|_, _, _| {}),
            )
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(requirement("Game detected", true, cx))
                    .child(requirement("Dump required", false, cx)),
            )
            .child({
                let e = e.clone();
                Collapsible::new("coll", "Danger Zone", !self.collapsed)
                    .detail("Config, CPU, latency, and VRAM controls")
                    .on_toggle(move |open, _, cx| {
                        e.update(cx, |g, cx| {
                            g.collapsed = !open;
                            cx.notify()
                        })
                    })
                    .child(
                        Button::new("dz", "Uninstall BO3 Enhanced")
                            .danger()
                            .icon(Icon::Trash2),
                    )
            })
            .child(
                DropZone::new("dz2", "Drop DUMP.zip or extracted folder here")
                    .detail("or browse to the dump source manually"),
            )
            .into_any_element()
    }

    fn overlays(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        let tp = |k: ToastKind, m: &'static str| {
            cx.listener(move |g: &mut Self, _, _, cx| g.toasts.update(cx, |t, cx| t.push(k, m, cx)))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                self.section("Toasts", cx).child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(
                            Button::new("ts1", "Info")
                                .on_click(tp(ToastKind::Info, "Checking for updates...")),
                        )
                        .child(
                            Button::new("ts2", "Success")
                                .on_click(tp(ToastKind::Success, "Gamertag updated")),
                        )
                        .child(
                            Button::new("ts3", "Warning")
                                .on_click(tp(ToastKind::Warning, "t7patch.conf is read-only")),
                        )
                        .child(Button::new("ts4", "Error").on_click(tp(
                            ToastKind::Error,
                            "Install failed: connection reset by peer",
                        ))),
                ),
            )
            .child(
                self.section("Dialogs", cx).child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(Button::new("d1", "Open dialog").on_click(cx.listener(
                            |g, _, _, cx| {
                                g.dialog = true;
                                cx.notify()
                            },
                        )))
                        .child(
                            Button::new("d2", "Confirm dialog")
                                .danger()
                                .on_click(cx.listener(|g, _, _, cx| {
                                    g.confirm = true;
                                    cx.notify()
                                })),
                        )
                        .child(
                            Button::new("d3", "Directory browser")
                                .icon(Icon::FolderOpen)
                                .on_click(cx.listener(|g, _, _, cx| {
                                    g.browser_open = true;
                                    g.browser.update(cx, |b, cx| b.navigate("", cx));
                                    cx.notify();
                                })),
                        ),
                ),
            )
            .child(
                self.section("Activity log", cx)
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(Button::new("l1", "Add line").small().on_click(cx.listener(
                                |g, _, _, cx| {
                                    g.log.update(cx, |l, cx| {
                                        let n = l.len();
                                        l.push(LogLevel::Success, format!("Appended line {n}"), cx)
                                    })
                                },
                            )))
                            .child(Button::new("l2", "Add 500").small().on_click(cx.listener(
                                |g, _, _, cx| {
                                    g.log.update(cx, |l, cx| {
                                        for i in 0..500 {
                                            l.push(LogLevel::Info, format!("bulk line {i}"), cx)
                                        }
                                    })
                                },
                            )))
                            .child(
                                Button::new("l3", "Copy all")
                                    .small()
                                    .icon(Icon::Clipboard)
                                    .on_click(cx.listener(|g, _, _, cx| {
                                        let l = g.log.clone();
                                        l.update(cx, |l, cx| l.copy_all(cx))
                                    })),
                            )
                            .child(
                                Button::new("l4", "Clear")
                                    .small()
                                    .icon(Icon::Eraser)
                                    .on_click(cx.listener(|g, _, _, cx| {
                                        g.log.update(cx, |l, cx| l.clear(cx))
                                    })),
                            ),
                    )
                    .child(div().h(px(200.)).child(self.log.clone())),
            )
            .child(
                div()
                    .text_color(t.text_muted)
                    .text_size(px(t.font_xs))
                    .child(format!("last event: {}", self.last_event)),
            )
            .into_any_element()
    }
}

impl Render for GalleryView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let body = match self.tab {
            0 => self.controls(cx),
            1 => self.inputs(cx),
            2 => self.display(cx),
            _ => self.overlays(cx),
        };
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(14.))
            .bg(t.bg)
            .child(SubTabs::new("gallery-tabs", TABS).selected(self.tab).on_change({
                let e = cx.entity();
                move |i, _, cx| e.update(cx, |g, cx| { g.tab = i; cx.notify() })
            }))
            .child(ScrollArea::new(("gallery-scroll", self.tab)).child(body))
            .when(self.dialog, |d| {
                d.child(
                    Dialog::new("g-dialog")
                        .title("Copy this")
                        .description("Run this command in the Steam console to download the compatible build.")
                        .width(px(480.))
                        .child(div().p(px(10.)).rounded(px(8.)).bg(t.field).font_family(t.font_mono).text_size(px(t.font_sm)).child("download_depot 311210 311211 7068545421122688603"))
                        .footer(Button::new("dg-cancel", "Cancel").on_click(cx.listener(|g, _, _, cx| { g.dialog = false; cx.notify() })))
                        .footer(Button::new("dg-copy", "Copy").icon(Icon::Clipboard))
                        .footer(Button::new("dg-go", "Continue").primary().icon(Icon::ExternalLink))
                        .on_close({
                            let e = cx.entity();
                            move |_, cx| e.update(cx, |g, cx| { g.dialog = false; cx.notify() })
                        }),
                )
            })
            .when(self.confirm, |d| {
                let (e1, e2) = (cx.entity(), cx.entity());
                d.child(confirm_dialog(
                    "g-confirm",
                    "Reset to stock?",
                    "This removes all installed mods and restores the original game files.",
                    "Reset",
                    true,
                    move |_, cx| e1.update(cx, |g, cx| { g.confirm = false; g.last_event = "confirmed".into(); cx.notify() }),
                    move |_, cx| e2.update(cx, |g, cx| { g.confirm = false; cx.notify() }),
                ))
            })
            .when(self.browser_open, |d| d.child(self.browser.clone()))
            .child(self.toasts.clone())
    }
}
