//! Application bootstrap: global setup, key bindings, the main window.

use std::sync::Arc;

use gpui::{
    actions, prelude::*, px, size, App, Application, Bounds, KeyBinding, Size, TitlebarOptions, WindowBounds,
    WindowOptions,
};
use patchops_core::Core;

use crate::controller::Controller;
use crate::state::{subscribe_logs, UiEvent};
use crate::theme::Theme;
use crate::views::Shell;
use crate::widgets::{self, Icon};

actions!(
    patchops,
    [Quit, NextView, PrevView, Go1, Go2, Go3, Go4, Go5, Go6, RefreshState]
);

/// Sidebar entries (legacy `navItems`). To add a tab: variant + `ALL` + label/icon here, a file in
/// `views/`, a field + match arm in `views/shell.rs`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    Dashboard,
    T7,
    ExeSwapper,
    Enhanced,
    Graphics,
    Tools,
    /// Widget gallery; debug builds only, listed when `PATCHOPS_GALLERY=1`.
    #[cfg(debug_assertions)]
    Gallery,
}

impl Nav {
    #[cfg(debug_assertions)]
    pub const ALL: [Nav; 7] =
        [Nav::Dashboard, Nav::T7, Nav::ExeSwapper, Nav::Enhanced, Nav::Graphics, Nav::Tools, Nav::Gallery];
    #[cfg(not(debug_assertions))]
    pub const ALL: [Nav; 6] = [Nav::Dashboard, Nav::T7, Nav::ExeSwapper, Nav::Enhanced, Nav::Graphics, Nav::Tools];

    pub fn visible(self) -> bool {
        match self {
            #[cfg(debug_assertions)]
            Nav::Gallery => crate::views::gallery::enabled(),
            _ => true,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Nav::Dashboard => "Dashboard",
            Nav::T7 => "T7 Patch",
            Nav::ExeSwapper => "EXE Swapper",
            Nav::Enhanced => "Enhanced",
            Nav::Graphics => "Graphics",
            Nav::Tools => "Tools",
            #[cfg(debug_assertions)]
            Nav::Gallery => "Gallery",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Nav::Dashboard => Icon::LayoutDashboard,
            Nav::T7 => Icon::ShieldCheck,
            Nav::ExeSwapper => Icon::RefreshCw,
            Nav::Enhanced => Icon::Gem,
            Nav::Graphics => Icon::Image,
            Nav::Tools => Icon::Wrench,
            #[cfg(debug_assertions)]
            Nav::Gallery => Icon::Eye,
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|n| *n == self).unwrap_or(0)
    }
}

/// Legacy window: 1320x860 (clamped to the primary display), minimum 800x600.
fn window_size(cx: &App) -> (Size<gpui::Pixels>, Size<gpui::Pixels>) {
    let display = cx.primary_display().map(|d| d.bounds().size);
    let clamp = |w: f32, h: f32| {
        let (dw, dh) = display.map(|s| (f32::from(s.width), f32::from(s.height))).unwrap_or((w, h));
        size(px(w.min(dw)), px(h.min(dh)))
    };
    (clamp(1320., 860.), clamp(800., 600.))
}

pub fn run(core: Arc<Core>, ui_rx: async_channel::Receiver<UiEvent>) {
    // Subscribe before anything can log so no entry is missed by the Activity Log.
    let log_rx = subscribe_logs(&core);
    Application::new().with_assets(widgets::Assets).run(move |cx: &mut App| {
        cx.set_global(Theme::dark());
        widgets::init(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("ctrl-tab", NextView, None),
            KeyBinding::new("ctrl-shift-tab", PrevView, None),
            KeyBinding::new("ctrl-1", Go1, None),
            KeyBinding::new("ctrl-2", Go2, None),
            KeyBinding::new("ctrl-3", Go3, None),
            KeyBinding::new("ctrl-4", Go4, None),
            KeyBinding::new("ctrl-5", Go5, None),
            KeyBinding::new("ctrl-6", Go6, None),
            KeyBinding::new("f5", RefreshState, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        // Quit when the last window closes.
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let (win, min) = window_size(cx);
        let bounds = Bounds::centered(None, win, cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(min),
                    titlebar: Some(TitlebarOptions { title: Some("PatchOpsIII".into()), ..Default::default() }),
                    // Lets a `.desktop` file (with the icon) match the window; gpui 0.2.2 has no
                    // runtime window-icon API.
                    app_id: Some("PatchOpsIII".into()),
                    ..Default::default()
                },
                move |_, cx| {
                    let controller = cx.new(|cx| Controller::new(core, ui_rx, log_rx, cx));
                    cx.new(|cx| Shell::new(controller, cx))
                },
            )
            .expect("open main window");
        window
            .update(cx, |view, window, cx| {
                window.focus(&gpui::Focusable::focus_handle(view, cx));
                cx.activate(true);
            })
            .ok();
    });
}
