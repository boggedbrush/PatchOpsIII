mod backend;
mod ui;

use anyhow::{Context, Result};
use gpui::{prelude::*, *};
use gpui_component::{Root, Theme, ThemeMode};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let url = std::env::var("PATCHOPSIII_GPUI_BACKEND_URL")
        .context("Start GPUI with: python scripts/gpui.py run (see native/gpui/README.md)")?;
    let backend = backend::Backend::start(url)?;
    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        let theme = Theme::global_mut(cx);
        theme.colors.primary = rgb(0xd92323).into();
        theme.colors.primary_hover = rgb(0xf03535).into();
        theme.colors.primary_active = rgb(0xa41616).into();
        theme.colors.primary_foreground = rgb(0xffffff).into();
        theme.colors.ring = rgb(0xf03535).into();
        cx.on_window_closed(|cx| {
            // GPUI 0.2.2's X11 close callback holds platform state borrowed.
            // Quit on a later event-loop turn, after that callback has returned.
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(1))
                    .await;
                let _ = cx.update(|cx| {
                    if cx.windows().is_empty() {
                        cx.quit();
                    }
                });
            })
            .detach();
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1150.), px(820.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(800.), px(600.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("PatchOpsIII · GPUI".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| ui::ControlCenter::new(backend, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("Unable to open the GPUI window");
        cx.activate(true);
    });
    Ok(())
}
