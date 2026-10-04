mod backend;
mod ui;

use anyhow::Result;
use gpui::{prelude::*, *};
use gpui_component::{Root, Theme, ThemeMode};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let backend = backend::Backend::start()?;
    // Read-only diagnostic used by archive smoke checks, without opening a window.
    if std::env::args().any(|argument| argument == "--smoke-status") {
        backend.requests.send(backend::Request {
            path: "/api/status".into(),
            body: None,
        })?;
        let reply = backend
            .replies
            .recv_timeout(std::time::Duration::from_secs(30))?;
        if reply.failed {
            anyhow::bail!("{}", reply.message);
        }
        println!(
            "{}",
            reply
                .state
                .ok_or_else(|| anyhow::anyhow!("Missing status document"))?
        );
        return Ok(());
    }
    let app = Application::new().with_assets(ui::Assets);
    app.run(move |cx: &mut App| {
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
        ui::log_session();
        cx.open_window(
            // Custom titlebar and client-side decorations where the platform
            // supports them; see `ui/chrome.rs`.
            ui::window_options(bounds),
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
