//! Window chrome: the custom titlebar, caption buttons, the rounded client-side
//! frame.
//!
//! The Electron app is frameless and draws its own titlebar (`TitleBar` in
//! `src/renderer/main.tsx`, `.titlebar`/`.window-controls` in `app.css`).
//! The native window does the same with the platform's own mechanics:
//!
//! * **Windows**: the strip is the real titlebar. `window_control_area`
//!   (Drag/Min/Max/Close) answers `WM_NCHITTEST`, so dragging, double-click,
//!   Aero snap, snap layouts on the maximise button and the caption-button
//!   clicks are all handled by the OS.
//! * **Linux (Wayland and X11)**: the strip is a client-side titlebar. Moves
//!   use `start_window_move`, double-click toggles maximise, right-click opens
//!   the compositor's window menu, and the caption buttons call
//!   minimize/zoom/remove. Resize edges, cursors and the shadow come from
//!   gpui-component's `Root`; this module adds the rounded corners and edge
//!   outline, which follow `window.window_decorations()` so they disappear
//!   when maximised/tiled and when the compositor insists on server-side
//!   decorations (then the caption buttons are hidden too).
//! * **macOS**: traffic lights (`appears_transparent`), no caption buttons.
use super::theme;
use gpui::{prelude::*, *};
use std::{cell::Cell, rc::Rc, sync::OnceLock};

pub const TITLEBAR_HEIGHT: f32 = 32.;
/// `.window-controls` is three 46px columns.
const CAPTION_WIDTH: f32 = 46.;
/// Corner radius of a client-decorated window.
const FRAME_RADIUS: f32 = 10.;

/// Test hook: `server` asks for compositor-drawn decorations (the fallback
/// path), `client` is the default on Linux.
const DECORATIONS_ENV: &str = "PATCHOPSIII_GPUI_DECORATIONS";
/// Test hook: set to `1` to open the window maximised.
const MAXIMIZED_ENV: &str = "PATCHOPSIII_GPUI_MAXIMIZED";

/// The windowing system GPUI is running on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    Windows,
    MacOs,
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    Wayland,
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    X11,
    Other,
}

impl Session {
    pub fn name(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::MacOs => "macOS",
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            Self::Wayland => "Wayland",
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            Self::X11 => "X11",
            Self::Other => "headless",
        }
    }
}

pub fn session() -> Session {
    static SESSION: OnceLock<Session> = OnceLock::new();
    *SESSION.get_or_init(|| {
        if cfg!(target_os = "windows") {
            Session::Windows
        } else if cfg!(target_os = "macos") {
            Session::MacOs
        } else {
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            match gpui::guess_compositor() {
                "Wayland" => return Session::Wayland,
                "X11" => return Session::X11,
                _ => {}
            }
            Session::Other
        }
    })
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
}

/// The app paints every pixel opaquely. Wayland makes client-decorated
/// surfaces transparent by itself; X11 needs the flag so the rounded corners
/// and shadow are cut out instead of rendering black.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn background_for(session: Session, csd: bool) -> WindowBackgroundAppearance {
    if session == Session::X11 && csd {
        WindowBackgroundAppearance::Transparent
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn background_for(_: Session, _: bool) -> WindowBackgroundAppearance {
    WindowBackgroundAppearance::Opaque
}

/// The options for the main window.
pub fn window_options(bounds: Bounds<Pixels>) -> WindowOptions {
    let session = session();
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    let linux = matches!(session, Session::Wayland | Session::X11);
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    let linux = false;
    WindowOptions {
        window_bounds: Some(if env_flag(MAXIMIZED_ENV) {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        }),
        window_min_size: Some(size(px(800.), px(600.))),
        titlebar: Some(TitlebarOptions {
            title: Some("PatchOpsIII".into()),
            // Windows/macOS: no OS titlebar; macOS keeps its traffic lights.
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(10.))),
        }),
        // Ask for client-side decorations; the compositor may refuse
        // (`settle` reads the outcome) and X11 without a compositor falls
        // back to server-side decorations by itself.
        window_decorations: linux.then_some(
            if std::env::var(DECORATIONS_ENV).is_ok_and(|value| value == "server") {
                WindowDecorations::Server
            } else {
                WindowDecorations::Client
            },
        ),
        window_background: background_for(session, true),
        ..Default::default()
    }
}

/// What the window ended up with, as read from the platform.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// The app draws the frame (and, on Linux, the caption buttons).
    pub csd: bool,
    pub tiling: Tiling,
    pub maximized: bool,
}

impl Frame {
    pub fn read(window: &Window) -> Self {
        let (csd, tiling) = match window.window_decorations() {
            Decorations::Client { tiling } => (true, tiling),
            Decorations::Server => (false, Tiling::default()),
        };
        Self {
            csd,
            tiling,
            maximized: window.is_maximized(),
        }
    }

    /// Whether the window draws its own caption buttons.
    fn caption_buttons(&self) -> bool {
        match session() {
            Session::Windows => true,
            Session::MacOs | Session::Other => false,
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            Session::Wayland | Session::X11 => self.csd,
        }
    }

    /// Round the corners that are not flush with a screen edge.
    pub fn round<E: Styled>(&self, mut element: E, radius: Pixels) -> E {
        if !self.csd {
            return element;
        }
        let tiling = self.tiling;
        if !(tiling.top || tiling.left) {
            element = element.rounded_tl(radius);
        }
        if !(tiling.top || tiling.right) {
            element = element.rounded_tr(radius);
        }
        if !(tiling.bottom || tiling.left) {
            element = element.rounded_bl(radius);
        }
        if !(tiling.bottom || tiling.right) {
            element = element.rounded_br(radius);
        }
        element
    }

    /// Rounded corners plus the 1px outline of a client-decorated window.
    pub fn frame<E: Styled>(&self, mut element: E) -> E {
        if !self.csd {
            return element;
        }
        let tiling = self.tiling;
        element = self
            .round(element, px(FRAME_RADIUS))
            .border_color(theme::window_edge());
        if !tiling.top {
            element = element.border_t_1();
        }
        if !tiling.bottom {
            element = element.border_b_1();
        }
        if !tiling.left {
            element = element.border_l_1();
        }
        if !tiling.right {
            element = element.border_r_1();
        }
        element
    }

    /// Round a full-window overlay (a modal scrim) like the frame it covers.
    pub fn round_overlay<E: Styled>(&self, element: E) -> E {
        self.round(element, px(FRAME_RADIUS - 1.))
    }
}

/// Apply the window background for the decorations the platform actually
/// granted, and re-apply it when they change (e.g. a compositor answering a
/// client-side-decoration request with server-side).
pub fn settle(window: &mut Window) {
    thread_local! {
        static APPLIED: Cell<Option<WindowBackgroundAppearance>> = const { Cell::new(None) };
    }
    let frame = Frame::read(window);
    let background = background_for(session(), frame.csd);
    if APPLIED.get() == Some(background) {
        return;
    }
    APPLIED.set(Some(background));
    window.set_background_appearance(background);
    info(format_args!(
        "window frame: {} decorations ({background:?})",
        if frame.csd {
            "client-side"
        } else {
            "server-side"
        },
    ));
}

/// An INFO line on stderr, shown when `RUST_LOG` enables info for this crate
/// (`RUST_LOG=info`, `debug`, `patchopsiii_gpui=info`, ...). The crate has no
/// direct `log` dependency, and the app's default filter is `warn`.
pub fn info(message: std::fmt::Arguments) {
    let enabled = std::env::var("RUST_LOG").is_ok_and(|filter| {
        filter.split(',').any(|directive| {
            let (target, level) = directive.rsplit_once('=').unwrap_or(("", directive));
            matches!(level.trim(), "info" | "debug" | "trace")
                && (target.is_empty() || "patchopsiii_gpui".starts_with(target.trim()))
        })
    });
    if enabled {
        eprintln!("[INFO  patchopsiii_gpui::ui::chrome] {message}");
    }
}

/// One line for the log: which windowing system GPUI picked.
pub fn log_session() {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    info(format_args!(
        "compositor: {} (desktop: {})",
        session().name(),
        if desktop.is_empty() {
            "unknown"
        } else {
            &desktop
        },
    ));
}

/// Shared between a titlebar's drag regions: set on mouse-down, consumed by
/// the first drag motion (or cleared on release), so a double-click still
/// reaches the maximise toggle.
pub type DragArmed = Rc<Cell<bool>>;

fn toggle_maximize(window: &mut Window) {
    if cfg!(target_os = "macos") {
        window.titlebar_double_click();
    } else {
        window.zoom_window();
    }
}

/// An area of the titlebar that moves the window.
pub fn drag_region(id: impl Into<ElementId>, armed: &DragArmed, frame: Frame) -> Stateful<Div> {
    let region = div().id(id).window_control_area(WindowControlArea::Drag);
    if cfg!(target_os = "windows") {
        // `WM_NCHITTEST` reports HTCAPTION: the OS owns move/double-click.
        return region;
    }
    let csd = frame.csd;
    region
        .on_mouse_down(MouseButton::Left, {
            let armed = armed.clone();
            move |event, window, _| {
                if event.click_count >= 2 {
                    armed.set(false);
                    toggle_maximize(window);
                } else {
                    armed.set(true);
                }
            }
        })
        .on_mouse_move({
            let armed = armed.clone();
            move |event, window, _| {
                if armed.get() && event.dragging() {
                    armed.set(false);
                    window.start_window_move();
                }
            }
        })
        .on_mouse_up(MouseButton::Left, {
            let armed = armed.clone();
            move |_, _, _| armed.set(false)
        })
        .on_mouse_up_out(MouseButton::Left, {
            let armed = armed.clone();
            move |_, _, _| armed.set(false)
        })
        .on_mouse_down(MouseButton::Right, move |event, window, _| {
            // The compositor's own window menu (restore/move/resize/close).
            if csd {
                window.show_window_menu(event.position);
            }
        })
}

/// The 32px strip: `.titlebar`.
pub fn titlebar(frame: Frame) -> Div {
    let mut bar = div()
        .flex()
        .items_center()
        .flex_none()
        .h(px(TITLEBAR_HEIGHT))
        .bg(theme::titlebar())
        .border_b_1()
        .border_color(theme::white(0.08))
        .when(cfg!(target_os = "macos"), |this| this.pl(px(80.)));
    if frame.csd {
        // The strip sits inside the 1px frame outline.
        let radius = px(FRAME_RADIUS - 1.);
        let tiling = frame.tiling;
        if !(tiling.top || tiling.left) {
            bar = bar.rounded_tl(radius);
        }
        if !(tiling.top || tiling.right) {
            bar = bar.rounded_tr(radius);
        }
    }
    bar
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Caption {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl Caption {
    fn path(self) -> &'static str {
        match self {
            Self::Minimize => "caption/minimize.svg",
            Self::Maximize => "caption/maximize.svg",
            Self::Restore => "caption/restore.svg",
            Self::Close => "caption/close.svg",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Minimize => "Minimize window",
            Self::Maximize => "Maximize window",
            Self::Restore => "Restore window",
            Self::Close => "Close window",
        }
    }

    fn area(self) -> WindowControlArea {
        match self {
            Self::Minimize => WindowControlArea::Min,
            Self::Maximize | Self::Restore => WindowControlArea::Max,
            Self::Close => WindowControlArea::Close,
        }
    }

    fn act(self, window: &mut Window) {
        match self {
            Self::Minimize => window.minimize_window(),
            Self::Maximize | Self::Restore => window.zoom_window(),
            Self::Close => window.remove_window(),
        }
    }
}

/// `.window-control`: 46x32, a 10px glyph, grey until hovered, red for close.
fn caption_button(kind: Caption, frame: Frame) -> Stateful<Div> {
    let close = kind == Caption::Close;
    let (hover_bg, hover_fg): (Hsla, Hsla) = if close {
        (rgb(0xc91b17).into(), rgb(0xffffff).into())
    } else {
        (theme::white(0.08), theme::text())
    };
    let mut button = div()
        .id(kind.label())
        .group("caption")
        .window_control_area(kind.area())
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .w(px(CAPTION_WIDTH))
        .h(px(TITLEBAR_HEIGHT))
        .hover(move |style| style.bg(hover_bg))
        .child(
            svg()
                .path(kind.path())
                .size(px(10.))
                .flex_none()
                .text_color(theme::text().alpha(0.76))
                .group_hover("caption", move |style| style.text_color(hover_fg)),
        );
    if close && frame.csd && !(frame.tiling.top || frame.tiling.right) {
        // The hover fill must not poke out of the window's rounded corner.
        button = button.rounded_tr(px(FRAME_RADIUS - 1.));
    }
    if cfg!(target_os = "windows") {
        // The OS performs the action for HTMINBUTTON/HTMAXBUTTON/HTCLOSE.
        button
    } else {
        button
            .cursor_pointer()
            .on_click(move |_, window, _| kind.act(window))
    }
}

/// `.window-controls`, when this window draws its own.
pub fn caption_buttons(frame: Frame, window: &Window) -> Option<Div> {
    if !frame.caption_buttons() {
        return None;
    }
    let controls = window.window_controls();
    let maximize = if frame.maximized {
        Caption::Restore
    } else {
        Caption::Maximize
    };
    Some(
        div()
            .flex()
            .flex_none()
            .h_full()
            .when(controls.minimize, |this| {
                this.child(caption_button(Caption::Minimize, frame))
            })
            .when(controls.maximize, |this| {
                this.child(caption_button(maximize, frame))
            })
            .child(caption_button(Caption::Close, frame)),
    )
}

/// Called from the first `render`. GPUI only draws once the compositor has
/// configured the surface and asked for a frame, so reaching this means the
/// window is mapped; the line is what `scripts/smoke_gpui_wayland.py` waits for.
/// With `PATCHOPSIII_GPUI_SMOKE_EXIT=1` the window then closes itself (a normal
/// window close, so the process exits through the usual path with status 0).
pub fn first_frame<T: 'static>(window: &Window, cx: &mut Context<T>) {
    let viewport = window.viewport_size();
    info(format_args!(
        "first frame: window mapped, {}x{} px viewport, {} decorations",
        f32::from(viewport.width) as i32,
        f32::from(viewport.height) as i32,
        if Frame::read(window).csd {
            "client-side"
        } else {
            "server-side"
        },
    ));
    if env_flag("PATCHOPSIII_GPUI_SMOKE_EXIT") {
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(1))
                .await;
            cx.update(|window, _| window.remove_window()).ok();
        })
        .detach();
    }
}
