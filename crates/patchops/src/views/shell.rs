//! The application shell (legacy `App` chrome in `main.tsx`): header with version / update check,
//! directory row, error strip, sidebar, the active tab, the Activity Log panel, and the global
//! overlays (confirm dialog, depot prompt, directory browser, toasts).

use std::sync::Arc;

use gpui::{
    div, img, prelude::*, px, AnyElement, App, Context, CursorStyle, Decorations, Entity, FocusHandle, Focusable,
    FontWeight, Image, ImageFormat, IntoElement, MouseButton, Render, ResizeEdge, SharedString, Subscription, Window,
};

use crate::actions::Action;
use crate::app::{Go1, Go2, Go3, Go4, Go5, Go6, Nav, NextView, PrevView, RefreshState};
use crate::controller::Controller;
use crate::state::Boot;
use crate::theme::ActiveTheme;
use crate::views::{
    DashboardView, EnhancedView, ExeSwapView, GraphicsView, T7View, ToolsView, PANEL_GAP,
};
#[cfg(debug_assertions)]
use crate::views::GalleryView;
use crate::widgets::{
    confirm_dialog, icon, Alert, Button, Dialog, Icon, NavItem, ProgressBar, Spinner, TabNav, TextInput,
};

static LOGO: &[u8] = include_bytes!("../../assets/img/logo.png");

/// Page gutter (legacy `--app-pad`) and gap between the rows of the shell (`--app-gap`).
const PAD: f32 = 14.0;
const GAP: f32 = 10.0;
const LOG_PANEL_H: f32 = 180.0;
/// Activity Log height in short windows (viewport height <= `SHORT_WINDOW`), so tabs keep room.
const LOG_PANEL_H_SHORT: f32 = 110.0;
const SHORT_WINDOW: f32 = 640.0;

pub struct Shell {
    c: Entity<Controller>,
    focus: FocusHandle,
    logo: Arc<Image>,
    path_input: Entity<TextInput>,
    dashboard: Entity<DashboardView>,
    t7: Entity<T7View>,
    exe_swap: Entity<ExeSwapView>,
    enhanced: Entity<EnhancedView>,
    graphics: Entity<GraphicsView>,
    tools: Entity<ToolsView>,
    #[cfg(debug_assertions)]
    gallery: Option<Entity<GalleryView>>,
    /// A dialog (confirm / depot prompt / directory browser) was open at the last render.
    overlay_open: bool,
    /// What had keyboard focus before the dialog took it; restored when the last one closes.
    prev_focus: Option<FocusHandle>,
    _sub: Subscription,
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Shell {
    pub fn new(controller: Entity<Controller>, cx: &mut Context<Self>) -> Self {
        let c = controller.clone();
        let path_input = cx.new(|cx| {
            let mut i = TextInput::new("Black Ops III directory", cx);
            i.set_read_only(true, cx);
            i
        });
        #[cfg(debug_assertions)]
        let gallery = crate::views::gallery::enabled().then(|| cx.new(GalleryView::new));
        #[cfg(debug_assertions)]
        if gallery.is_some() {
            controller.update(cx, |c, cx| c.navigate(Nav::Gallery, cx));
        }
        Self {
            focus: cx.focus_handle(),
            logo: Arc::new(Image::from_bytes(ImageFormat::Png, LOGO.to_vec())),
            path_input,
            dashboard: cx.new(|cx| DashboardView::new(c.clone(), cx)),
            t7: cx.new(|cx| T7View::new(c.clone(), cx)),
            exe_swap: cx.new(|cx| ExeSwapView::new(c.clone(), cx)),
            enhanced: cx.new(|cx| EnhancedView::new(c.clone(), cx)),
            graphics: cx.new(|cx| GraphicsView::new(c.clone(), cx)),
            tools: cx.new(|cx| ToolsView::new(c.clone(), cx)),
            #[cfg(debug_assertions)]
            gallery,
            overlay_open: false,
            prev_focus: None,
            _sub: cx.observe(&controller, |_, _, cx| cx.notify()),
            c: controller,
        }
    }

    /// Keyboard navigation: switch tab and keep keyboard focus on the shell root, so the next
    /// shortcut still dispatches even though the focused widget of the old tab is gone.
    fn go_key(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.go(nav, cx);
    }

    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let n = Nav::ALL.len() as isize;
        let mut i = self.c.read(cx).nav().index() as isize;
        loop {
            i = (i + delta).rem_euclid(n);
            if Nav::ALL[i as usize].visible() {
                break;
            }
        }
        self.go_key(Nav::ALL[i as usize], window, cx);
    }

    fn go(&mut self, nav: Nav, cx: &mut Context<Self>) {
        self.c.update(cx, |c, cx| c.navigate(nav, cx));
    }

    fn startup(&self, failed: Option<&str>, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let c = self.c.clone();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .child(img(self.logo.clone()).size(px(64.)).rounded(px(14.)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(5.))
                    .child(
                        div()
                            .text_size(px(t.font_lg))
                            .font_weight(FontWeight::BOLD)
                            .child(if failed.is_some() { "Startup took too long" } else { "Opening PatchOpsIII" }),
                    )
                    .child(
                        div()
                            .text_size(px(t.font_sm))
                            .text_color(t.text_muted)
                            .child(match failed {
                                Some(m) => SharedString::from(m.to_string()),
                                None => "Loading your settings...".into(),
                            }),
                    ),
            )
            .when(failed.is_none(), |d| d.child(ProgressBar::new("startup").value(None).width(px(132.))))
            .when(failed.is_some(), |d| {
                d.child(Button::new("startup-retry", "Try Again").icon(Icon::RefreshCw).on_click(
                    move |_, _, cx| c.update(cx, |c, cx| c.retry(cx)),
                ))
            })
    }

    fn header(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        // Client-side decorations (GNOME/Mutter on Wayland offers none): draw the caption buttons
        // and make the header the drag handle. Server-side (X11 WMs, KDE, sway): nothing extra.
        let csd = matches!(window.window_decorations(), Decorations::Client { .. });
        let controls = window.window_controls();
        let maximized = window.is_maximized();
        let ctl = self.c.read(cx);
        let (busy, queued) = (ctl.any_busy(), ctl.queued());
        let version = ctl.state().map(|s| s.app_version.clone()).unwrap_or_else(|| patchops_core::version().to_string());
        let version = if version.to_lowercase().starts_with('v') { version } else { format!("v{version}") };
        let disabled = !ctl.ready() || ctl.is_busy("update-check");
        let c = self.c.clone();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(32.))
            .px(px(12.))
            .bg(gpui::rgb(0x050505))
            .border_b_1()
            .border_color(gpui::white().opacity(0.08))
            .when(csd, |d| {
                d.on_mouse_down(MouseButton::Left, |e, w, _| {
                    if e.click_count == 2 {
                        w.zoom_window();
                    } else {
                        w.start_window_move();
                    }
                })
                .on_mouse_down(MouseButton::Right, |e, w, _| w.show_window_menu(e.position))
            })
            .child(img(self.logo.clone()).size(px(18.)).rounded(px(4.)))
            .child(div().text_size(px(13.)).child("PatchOpsIII"))
            .child(
                div()
                    .id("update-check")
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .px(px(6.))
                    .h(px(22.))
                    .rounded(px(5.))
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .tab_index(0)
                    .focus(move |s| s.bg(t.hover))
                    .when(csd, |d| d.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))
                    .when(!disabled, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(t.hover).text_color(t.text))
                            .on_click(move |_, _, cx| c.update(cx, |c, cx| c.run(Action::UpdateCheck, cx)))
                    })
                    .when(disabled, |d| d.opacity(0.5))
                    .child(version)
                    .child(icon(Icon::RefreshCw).size(px(13.)).text_color(t.text_muted)),
            )
            .child(div().flex_1())
            // Visible feedback for queued work (ops run one at a time).
            .when(busy, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(t.font_xs))
                        .text_color(t.text_muted)
                        .child(Spinner::new("header-spin").size(px(12.)).color(t.text_muted))
                        .child(if queued > 0 { format!("Working... ({queued} queued)") } else { "Working...".to_string() }),
                )
            })
            .when(csd, |d| {
                d.child(
                    div()
                        .flex()
                        .h_full()
                        .ml(px(6.))
                        .mr(px(-12.))
                        .when(controls.minimize, |d| {
                            d.child(caption_button("cap-min", Icon::Minus, false, |_, w, _| w.minimize_window()))
                        })
                        .when(controls.maximize, |d| {
                            d.child(caption_button(
                                "cap-max",
                                if maximized { Icon::Copy } else { Icon::Square },
                                false,
                                |_, w, _| w.zoom_window(),
                            ))
                        })
                        .child(caption_button("cap-close", Icon::X, true, |_, w, _| w.remove_window())),
                )
            })
    }

    fn directory_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let ctl = self.c.read(cx);
        let dir = ctl.state().and_then(|s| s.game_dir.clone()).unwrap_or_default();
        let ready = ctl.ready();
        let launching = ctl.is_busy("launch");
        self.path_input.update(cx, |i, cx| i.sync_text(&dir, cx));
        let c = self.c.clone();
        let c2 = self.c.clone();
        div()
            .flex()
            .flex_none()
            .items_end()
            .gap(px(12.))
            .pt(px(10.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(t.font_xs))
                            .font_weight(FontWeight::BOLD)
                            .text_color(t.text_muted)
                            .child("Game Directory:"),
                    )
                    .child(self.path_input.clone()),
            )
            .child(
                Button::new("browse", "Browse...")
                    .icon(Icon::FolderOpen)
                    .disabled(!ready)
                    .on_click(move |_, _, cx| {
                        c.update(cx, |c, cx| c.open_browser(crate::controller::BrowseTarget::GameDir, None, cx))
                    }),
            )
            .child(
                div().min_w(px(148.)).child(
                    Button::new("launch", "Launch Game")
                        .primary()
                        .icon(Icon::SquarePlay)
                        .full_width()
                        .disabled(!ready)
                        .busy(launching)
                        .on_click(move |_, _, cx| c2.update(cx, |c, cx| c.run(Action::Launch, cx))),
                ),
            )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let nav = self.c.read(cx).nav();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(7.))
            .w(px(t.nav_w))
            .h_full()
            .p(px(10.))
            .border_1()
            .border_color(t.border)
            .rounded(px(t.radius))
            .bg(t.sidebar)
            .children(Nav::ALL.into_iter().filter(|n| n.visible()).map(|item| {
                NavItem::new(SharedString::from(format!("nav-{}", item.label())), item.icon(), item.label())
                    .active(item == nav)
                    .on_click(cx.listener(move |this, _, _, cx| this.go(item, cx)))
            }))
    }

    fn content(&self, nav: Nav) -> gpui::AnyElement {
        match nav {
            Nav::Dashboard => self.dashboard.clone().into_any_element(),
            Nav::T7 => self.t7.clone().into_any_element(),
            Nav::ExeSwapper => self.exe_swap.clone().into_any_element(),
            Nav::Enhanced => self.enhanced.clone().into_any_element(),
            Nav::Graphics => self.graphics.clone().into_any_element(),
            Nav::Tools => self.tools.clone().into_any_element(),
            #[cfg(debug_assertions)]
            Nav::Gallery => match &self.gallery {
                Some(g) => g.clone().into_any_element(),
                None => div().into_any_element(),
            },
        }
    }

    fn log_panel(&self, short: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let logs = self.c.read(cx).logs().clone();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .h(px(if short { LOG_PANEL_H_SHORT } else { LOG_PANEL_H }))
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(t.font_lg))
                    .font_weight(FontWeight::BOLD)
                    .child("Activity Log"),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(10.))
                    .border_1()
                    .border_color(t.border)
                    .rounded(px(t.radius))
                    .bg(t.panel)
                    .child(logs),
            )
    }

    /// Confirm / depot dialogs of the controller.
    fn overlays(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t = *cx.theme();
        let mut out = Vec::new();
        let ctl = self.c.read(cx);
        if let Some(cf) = ctl.confirm_dialog().cloned() {
            let (a, b) = (self.c.clone(), self.c.clone());
            out.push(
                confirm_dialog(
                    "confirm",
                    cf.title,
                    cf.message,
                    cf.confirm_label,
                    cf.destructive,
                    move |_, cx| a.update(cx, |c, cx| c.resolve_confirm(true, cx)),
                    move |_, cx| b.update(cx, |c, cx| c.resolve_confirm(false, cx)),
                )
                .into_any_element(),
            );
        }
        if let Some(d) = ctl.depot_prompt().cloned() {
            let (c1, c2, c3, c4) = (self.c.clone(), self.c.clone(), self.c.clone(), self.c.clone());
            out.push(
                Dialog::new("depot")
                    .title("Copy this")
                    .description("Run this command in the Steam console to download the compatible build.")
                    .width(px(520.))
                    .no_close_button()
                    // Legacy: a click on the backdrop did nothing; Esc cancels. Start on Continue.
                    .backdrop_closes(false)
                    .initial_focus(2)
                    .on_close(move |_, cx| c1.update(cx, |c, cx| c.depot_cancel(cx)))
                    .child(
                        div()
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(t.border)
                            .bg(gpui::black().opacity(0.34))
                            .font_family(t.font_mono)
                            .text_size(px(t.font_sm))
                            .child(d.command.clone()),
                    )
                    .when(d.watching, |dlg| {
                        dlg.child(
                            div()
                                .text_size(px(t.font_sm))
                                .text_color(t.text_muted)
                                .child("Watching for the depot. PatchOpsIII will install it when ready."),
                        )
                    })
                    .footer(
                        Button::new("depot-cancel", "Cancel").on_click(move |_, _, cx| c2.update(cx, |c, cx| c.depot_cancel(cx))),
                    )
                    .footer(
                        Button::new("depot-copy", if d.copied { "Copied" } else { "Copy" })
                            .icon(Icon::Clipboard)
                            .on_click(move |_, _, cx| c3.update(cx, |c, cx| c.depot_copy(cx))),
                    )
                    .footer(
                        Button::new("depot-continue", "Continue")
                            .primary()
                            .icon(Icon::ExternalLink)
                            .disabled(d.watching)
                            .on_click(move |_, _, cx| c4.update(cx, |c, cx| c.depot_continue(cx))),
                    )
                    .into_any_element(),
            );
        }
        out
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::views::trace_first_render();
        let t = *cx.theme();
        let (nav, has_state, boot, error, browser, toasts) = {
            let c = self.c.read(cx);
            (
                c.nav(),
                c.state().is_some(),
                c.boot_status().clone(),
                c.error().cloned(),
                c.browser().cloned(),
                c.toasts().clone(),
            )
        };

        // Dialogs take keyboard focus (their card). When the last one closes, give it back to
        // what had it before; if that control is gone (or nothing had focus) fall back to the
        // root, otherwise the global shortcuts (Ctrl+1.., F5) would stop dispatching.
        let overlay_open = {
            let c = self.c.read(cx);
            c.confirm_dialog().is_some() || c.depot_prompt().is_some() || browser.is_some()
        };
        if overlay_open && !self.overlay_open {
            self.prev_focus = window.focused(cx).filter(|f| *f != self.focus);
        } else if !overlay_open && self.overlay_open {
            match self.prev_focus.take() {
                Some(h) => window.focus(&h),
                None => window.focus(&self.focus),
            }
            // The remembered control may have left the tree meanwhile (tab switch, rebuilt view):
            // verify on the next frame that shortcuts still reach the root.
            let root = self.focus.clone();
            window.on_next_frame(move |w, cx| {
                if !w.is_action_available(&RefreshState, cx) {
                    w.focus(&root);
                }
            });
        }
        self.overlay_open = overlay_open;
        let csd = matches!(window.window_decorations(), Decorations::Client { .. }) && !window.is_maximized();
        let short_window = f32::from(window.viewport_size().height) <= SHORT_WINDOW;

        let body = if !has_state {
            let failed = match &boot {
                Boot::Failed(m) => Some(m.as_str()),
                _ => None,
            };
            self.startup(failed, cx).into_any_element()
        } else {
            let c = self.c.clone();
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap(px(GAP))
                .px(px(PAD))
                .pb(px(PAD))
                .child(self.directory_row(cx))
                .when_some(error, |d, msg| {
                    d.child(Alert::error(msg).on_dismiss(move |_, _, cx| c.update(cx, |c, cx| c.dismiss_error(cx))))
                })
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .gap(px(PANEL_GAP))
                        .child(self.sidebar(cx))
                        .child(div().flex_1().min_w_0().min_h_0().flex().flex_col().child(self.content(nav))),
                )
                .child(self.log_panel(short_window, cx))
                .into_any_element()
        };

        div()
            .key_context("Root")
            .tab_nav()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NextView, w, cx| this.step(1, w, cx)))
            .on_action(cx.listener(|this, _: &PrevView, w, cx| this.step(-1, w, cx)))
            .on_action(cx.listener(|this, _: &Go1, w, cx| this.go_key(Nav::Dashboard, w, cx)))
            .on_action(cx.listener(|this, _: &Go2, w, cx| this.go_key(Nav::T7, w, cx)))
            .on_action(cx.listener(|this, _: &Go3, w, cx| this.go_key(Nav::ExeSwapper, w, cx)))
            .on_action(cx.listener(|this, _: &Go4, w, cx| this.go_key(Nav::Enhanced, w, cx)))
            .on_action(cx.listener(|this, _: &Go5, w, cx| this.go_key(Nav::Graphics, w, cx)))
            .on_action(cx.listener(|this, _: &Go6, w, cx| this.go_key(Nav::Tools, w, cx)))
            .on_action(cx.listener(|this, _: &RefreshState, _, cx| {
                this.c.update(cx, |c, cx| c.run(Action::Refresh, cx))
            }))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(t.bg_deep, 0.),
                gpui::linear_color_stop(t.bg, 1.),
            ))
            .text_color(t.text)
            .font_family(t.font_ui)
            .text_size(px(t.font_md))
            .child(self.header(window, cx))
            .child(body)
            .when(csd, |d| d.children(resize_zones()))
            .children(self.overlays(cx))
            .children(browser)
            .child(toasts)
    }
}

/// One client-side caption button (minimise / maximise-restore / close) of the header.
fn caption_button(
    id: &'static str,
    ic: Icon,
    danger: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(42.))
        .h_full()
        .cursor_pointer()
        .hover(move |s| {
            if danger {
                s.bg(gpui::rgb(0xc42b1c))
            } else {
                s.bg(gpui::white().opacity(0.1))
            }
        })
        // The header starts a window move on mouse-down; a button must not.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
        .child(icon(ic).size(px(14.)).text_color(gpui::white().opacity(0.85)))
}

/// Thin invisible hit zones along the window edges/corners that start a compositor resize
/// (client-side decorations only: with server-side ones the WM owns the borders).
fn resize_zones() -> Vec<AnyElement> {
    const E: f32 = 5.0;
    const C: f32 = 10.0;
    let zone = |id: &'static str, edge: ResizeEdge, cursor: CursorStyle| {
        div()
            .id(id)
            .absolute()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, move |_, w, cx| {
                w.start_window_resize(edge);
                cx.stop_propagation();
            })
    };
    vec![
        zone("rz-t", ResizeEdge::Top, CursorStyle::ResizeUpDown).top_0().left(px(C)).right(px(C)).h(px(E)).into_any_element(),
        zone("rz-b", ResizeEdge::Bottom, CursorStyle::ResizeUpDown).bottom_0().left(px(C)).right(px(C)).h(px(E)).into_any_element(),
        zone("rz-l", ResizeEdge::Left, CursorStyle::ResizeLeftRight).left_0().top(px(C)).bottom(px(C)).w(px(E)).into_any_element(),
        zone("rz-r", ResizeEdge::Right, CursorStyle::ResizeLeftRight).right_0().top(px(C)).bottom(px(C)).w(px(E)).into_any_element(),
        zone("rz-tl", ResizeEdge::TopLeft, CursorStyle::ResizeUpLeftDownRight).top_0().left_0().size(px(C)).into_any_element(),
        zone("rz-tr", ResizeEdge::TopRight, CursorStyle::ResizeUpRightDownLeft).top_0().right_0().size(px(C)).into_any_element(),
        zone("rz-bl", ResizeEdge::BottomLeft, CursorStyle::ResizeUpRightDownLeft).bottom_0().left_0().size(px(C)).into_any_element(),
        zone("rz-br", ResizeEdge::BottomRight, CursorStyle::ResizeUpLeftDownRight).bottom_0().right_0().size(px(C)).into_any_element(),
    ]
}
