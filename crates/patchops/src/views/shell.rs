//! The application shell (legacy `App` chrome in `main.tsx`): header with version / update check,
//! directory row, error strip, sidebar, the active tab, the Activity Log panel, and the global
//! overlays (confirm dialog, depot prompt, directory browser, toasts).

use std::sync::Arc;

use gpui::{
    div, img, prelude::*, px, App, Context, Entity, FocusHandle, Focusable, FontWeight, Image, ImageFormat,
    IntoElement, Render, SharedString, Subscription, Window,
};

use crate::actions::Action;
use crate::app::{Go1, Go2, Go3, Go4, Go5, Go6, Nav, NextView, PrevView, RefreshState};
use crate::controller::Controller;
use crate::state::Boot;
use crate::theme::ActiveTheme;
use crate::views::{
    DashboardView, EnhancedView, ExeSwapView, GalleryView, GraphicsView, T7View, ToolsView, PANEL_GAP,
};
use crate::widgets::{
    confirm_dialog, icon, Alert, Button, Dialog, Icon, NavItem, ProgressBar, TabNav, TextInput,
};

static LOGO: &[u8] = include_bytes!("../../assets/img/logo.png");

/// Page gutter (legacy `--app-pad`) and gap between the rows of the shell (`--app-gap`).
const PAD: f32 = 14.0;
const GAP: f32 = 10.0;
const LOG_PANEL_H: f32 = 180.0;

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
    gallery: Option<Entity<GalleryView>>,
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
        let gallery = crate::views::gallery::enabled().then(|| cx.new(GalleryView::new));
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
            gallery,
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

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let ctl = self.c.read(cx);
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
                    .when(!disabled, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(t.hover).text_color(t.text))
                            .on_click(move |_, _, cx| c.update(cx, |c, cx| c.run(Action::UpdateCheck, cx)))
                    })
                    .when(disabled, |d| d.opacity(0.5))
                    .child(version)
                    .child(icon(Icon::RefreshCw).size(px(13.)).text_color(t.text_muted)),
            )
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
            Nav::Gallery => match &self.gallery {
                Some(g) => g.clone().into_any_element(),
                None => div().into_any_element(),
            },
        }
    }

    fn log_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let logs = self.c.read(cx).logs().clone();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .h(px(LOG_PANEL_H))
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                .child(self.log_panel(cx))
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
            .child(self.header(cx))
            .child(body)
            .children(self.overlays(cx))
            .children(browser)
            .child(toasts)
    }
}
