//! One file per sidebar tab, plus the application shell.
//!
//! * `shell.rs`: window chrome (header, directory row, error strip, sidebar, Activity Log) and
//!   the global overlays (confirm / depot / directory browser dialogs, toasts).
//! * `dashboard.rs`: the reference implementation of the view conventions.
//! * `t7.rs`, `exe_swap.rs`, `enhanced.rs`, `graphics.rs`, `tools.rs`: one tab each.
//! * `gallery.rs`: widget gallery (`PATCHOPS_GALLERY=1`).
//!
//! Conventions: `GPUI_NOTES.md`, section "Writing a view".

pub mod dashboard;
pub mod enhanced;
pub mod exe_swap;
pub mod gallery;
pub mod graphics;
mod shell;
pub mod t7;
pub mod tools;

pub use dashboard::DashboardView;
pub use enhanced::EnhancedView;
pub use exe_swap::ExeSwapView;
pub use gallery::GalleryView;
pub use graphics::GraphicsView;
pub use shell::Shell;
pub use t7::T7View;
pub use tools::ToolsView;

use gpui::{App, ClickEvent, Context, Entity, Subscription, Window};

use crate::actions::Action;
use crate::controller::Controller;

/// Gap between panels in a tab (legacy `--panel-gap`).
pub const PANEL_GAP: f32 = 12.0;

/// Re-render `cx`'s view whenever the controller changes (state, busy flags, errors, logs).
/// Keep the returned subscription in a `_sub` field.
///
/// ```ignore
/// let _sub = views::watch(&controller, cx);
/// ```
pub fn watch<T: 'static>(controller: &Entity<Controller>, cx: &mut Context<T>) -> Subscription {
    cx.observe(controller, |_, _, cx| cx.notify())
}

/// `on_click` handler that runs `action` (cloned per click). One line per button:
/// `Button::new("x", "Launch").on_click(views::run(&c, Action::Launch))`.
pub fn run(controller: &Entity<Controller>, action: Action) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let c = controller.clone();
    move |_, _, cx| {
        let a = action.clone();
        c.update(cx, |c, cx| c.run(a, cx))
    }
}

/// Like [`run`], but asks the legacy confirmation first when the action has one
/// (uninstalls, clear mod files, reset to stock).
pub fn run_confirmed(
    controller: &Entity<Controller>,
    action: Action,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let c = controller.clone();
    move |_, _, cx| {
        let a = action.clone();
        c.update(cx, |c, cx| c.run_confirmed(a, cx))
    }
}

/// `on_change` handler for `Checkbox` / `Toggle`: builds the action from the new value.
/// `Checkbox::new("x", on).on_change(views::run_bool(&c, Action::IntroSkip))`
pub fn run_bool(
    controller: &Entity<Controller>,
    make: impl Fn(bool) -> Action + 'static,
) -> impl Fn(bool, &mut Window, &mut App) + 'static {
    let c = controller.clone();
    move |v, _, cx| {
        let a = make(v);
        c.update(cx, |c, cx| c.run(a, cx))
    }
}

/// `PATCHOPS_TRACE_STARTUP=1` prints the time to the first render to stderr.
pub(crate) fn trace_first_render() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if std::env::var_os("PATCHOPS_TRACE_STARTUP").is_some() {
            if let Some(t0) = crate::START.get() {
                eprintln!("first render at {:?} after process start", t0.elapsed());
            }
        }
    });
}
