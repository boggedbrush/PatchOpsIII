//! Root view, navigation and application bootstrap.

use std::sync::Arc;

use gpui::{
    actions, div, prelude::*, px, size, App, Application, Bounds, Context, Entity, FocusHandle,
    Focusable, KeyBinding, SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions,
};

use crate::state::{SharedState, UiEvent};
use crate::theme::{ActiveTheme, Theme};
use crate::views::{AboutView, DashboardView, LogsView, TasksView};
use crate::widgets;

actions!(patchops, [Quit, NextView, PrevView]);

/// Sidebar entries. Add a variant + a view file under `views/` to add a tab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    Dashboard,
    Logs,
    Tasks,
    About,
}

impl Nav {
    pub const ALL: [Nav; 4] = [Nav::Dashboard, Nav::Logs, Nav::Tasks, Nav::About];

    pub fn label(self) -> &'static str {
        match self {
            Nav::Dashboard => "Dashboard",
            Nav::Logs => "Logs",
            Nav::Tasks => "Tasks",
            Nav::About => "About",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|n| *n == self).unwrap_or(0)
    }
}

pub fn run(shared: Arc<SharedState>, ui_rx: async_channel::Receiver<UiEvent>) {
    Application::new().run(move |cx: &mut App| {
        cx.set_global(Theme::dark());
        widgets::init(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("ctrl-tab", NextView, None),
            KeyBinding::new("ctrl-shift-tab", PrevView, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        // Quit when the last window closes.
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1000.), px(680.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("PatchOpsIII".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |_, cx| cx.new(|cx| AppView::new(shared, ui_rx, cx)),
            )
            .expect("open main window");
        window
            .update(cx, |view, window, cx| {
                window.focus(&view.focus_handle(cx));
                cx.activate(true);
            })
            .ok();
    });
}

fn trace_first_render() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if std::env::var_os("PATCHOPS_TRACE_STARTUP").is_some() {
            if let Some(t0) = crate::START.get() {
                eprintln!("first render at {:?} after process start", t0.elapsed());
            }
        }
    });
}

pub struct AppView {
    nav: Nav,
    focus_handle: FocusHandle,
    dashboard: Entity<DashboardView>,
    logs: Entity<LogsView>,
    tasks: Entity<TasksView>,
    about: Entity<AboutView>,
}

impl Focusable for AppView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl AppView {
    fn new(
        shared: Arc<SharedState>,
        ui_rx: async_channel::Receiver<UiEvent>,
        cx: &mut Context<Self>,
    ) -> Self {
        // Background-thread -> UI bridge: the RPC thread / workers push `UiEvent`s
        // into a channel; this foreground task awaits them and updates the view.
        cx.spawn(async move |this, cx| {
            while let Ok(ev) = ui_rx.recv().await {
                let res = this.update(cx, |this, cx| {
                    if let UiEvent::Navigate(i) = ev {
                        this.nav = Nav::ALL[i.min(Nav::ALL.len() - 1)];
                    }
                    cx.notify();
                });
                if res.is_err() {
                    break; // view dropped
                }
            }
        })
        .detach();

        Self {
            nav: Nav::Dashboard,
            focus_handle: cx.focus_handle(),
            dashboard: cx.new(|cx| DashboardView::new(shared.clone(), cx)),
            logs: cx.new(|cx| LogsView::new(shared.clone(), cx)),
            tasks: cx.new(|cx| TasksView::new(shared.clone(), cx)),
            about: cx.new(|_| AboutView),
        }
    }

    fn go(&mut self, nav: Nav, cx: &mut Context<Self>) {
        self.nav = nav;
        cx.notify();
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = Nav::ALL.len() as isize;
        let i = (self.nav.index() as isize + delta).rem_euclid(n) as usize;
        self.go(Nav::ALL[i], cx);
    }
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        trace_first_render();
        let t = cx.theme().clone();
        let nav = self.nav;
        let sidebar = div()
            .w(px(180.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .bg(t.sidebar)
            .border_r_1()
            .border_color(t.border)
            .child(
                div()
                    .px_2()
                    .py_2()
                    .text_lg()
                    .text_color(t.text)
                    .child("PatchOpsIII"),
            )
            .children(Nav::ALL.map(|item| {
                let active = item == nav;
                let hover = t.hover;
                div()
                    .id(SharedString::from(format!("nav-{}", item.label())))
                    .px_3()
                    .py_2()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_color(if active { t.text } else { t.text_muted })
                    .when(active, |d| d.bg(t.panel))
                    .when(!active, |d| d.hover(move |s| s.bg(hover)))
                    .child(item.label())
                    .on_click(cx.listener(move |this, _, _, cx| this.go(item, cx)))
            }));

        let content = div().relative().flex_1().h_full().overflow_hidden().child(match nav {
            Nav::Dashboard => self.dashboard.clone().into_any_element(),
            Nav::Logs => self.logs.clone().into_any_element(),
            Nav::Tasks => self.tasks.clone().into_any_element(),
            Nav::About => self.about.clone().into_any_element(),
        });

        div()
            .key_context("Root")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &NextView, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &PrevView, _, cx| this.step(-1, cx)))
            .size_full()
            .flex()
            .flex_row()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(t.font_ui)
            .text_sm()
            .child(sidebar)
            .child(content)
    }
}
