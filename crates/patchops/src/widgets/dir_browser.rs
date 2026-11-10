//! Directory browser dialog (legacy "Select Game Directory" modal): a locations list, a path
//! field with Go, an up/".." row, and a folder list. The widget owns no filesystem access: it
//! emits `Navigate(path)` and the owner answers with `set_data` (from core's `browse` op).

use gpui::{
    div, prelude::*, px, uniform_list, App, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, IntoElement, Render, SharedString, Subscription, UniformListScrollHandle, Window,
};

use super::button::Button;
use super::icon::{icon, Icon};
use super::modal::Dialog;
use super::scroll::Scrollbar;
use super::text_input::{TextInput, TextInputEvent};
use crate::theme::ActiveTheme;

/// A named shortcut or drive root.
#[derive(Clone, Debug, Default)]
pub struct BrowseLocation {
    pub label: String,
    pub path: String,
}

#[derive(Clone, Debug, Default)]
pub struct BrowseEntry {
    pub name: String,
    pub path: String,
    /// Folder contains the game executable (shows a "BO3" tag).
    pub has_game_executable: bool,
}

/// Listing of one folder; mirrors the core `browse` op result.
#[derive(Clone, Debug, Default)]
pub struct BrowseData {
    pub path: String,
    pub parent: Option<String>,
    pub has_game_executable: bool,
    pub roots: Vec<BrowseLocation>,
    pub shortcuts: Vec<BrowseLocation>,
    pub entries: Vec<BrowseEntry>,
}

pub enum DirBrowserEvent {
    /// Owner should list this path and call `set_data` (or `set_error`).
    Navigate(String),
    /// "Use Selected Folder" pressed for this path.
    Select(String),
    /// Cancel / X / Esc / backdrop.
    Cancel,
}

/// Stateful dialog. Show it by adding the entity as the last child of a `relative()` root while
/// open. Typical flow:
///
/// ```ignore
/// let b = cx.new(|cx| DirBrowser::new("Select Game Directory",
///     "Choose the folder that contains BlackOps3.exe.", "Use Selected Folder", cx)
///     .require_game_executable(true));
/// cx.subscribe(&b, |this, b, ev: &DirBrowserEvent, cx| match ev {
///     DirBrowserEvent::Navigate(p) => this.spawn_browse(p.clone(), b.clone(), cx), // core "browse" -> b.update(.., |b, cx| b.set_data(d, cx))
///     DirBrowserEvent::Select(p) => { this.set_game_dir(p); this.browse_open = false; cx.notify(); }
///     DirBrowserEvent::Cancel => { this.browse_open = false; cx.notify(); }
/// }).detach();
/// b.update(cx, |b, cx| b.navigate("", cx));     // when opening: "" = default location
/// ```
pub struct DirBrowser {
    title: SharedString,
    description: SharedString,
    confirm_label: SharedString,
    require_exe: bool,
    path_input: Entity<TextInput>,
    data: Option<BrowseData>,
    error: Option<SharedString>,
    loading: bool,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    _sub: Subscription,
}

impl EventEmitter<DirBrowserEvent> for DirBrowser {}

impl Focusable for DirBrowser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DirBrowser {
    pub fn new(
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let path_input = cx.new(|cx| TextInput::new("Type a path and press Enter", cx));
        let sub = cx.subscribe(&path_input, |this, input, ev: &TextInputEvent, cx| {
            if let TextInputEvent::Submit(p) = ev {
                let _ = input;
                this.navigate(p.to_string(), cx);
            }
        });
        Self {
            title: title.into(),
            description: description.into(),
            confirm_label: confirm_label.into(),
            require_exe: false,
            path_input,
            data: None,
            error: None,
            loading: false,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            _sub: sub,
        }
    }

    /// Confirm is only enabled in folders that contain the game executable.
    pub fn require_game_executable(mut self, yes: bool) -> Self {
        self.require_exe = yes;
        self
    }

    /// Ask the owner to list `path` (empty = the owner's default start folder).
    pub fn navigate(&mut self, path: impl Into<String>, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.emit(DirBrowserEvent::Navigate(path.into()));
        cx.notify();
    }

    pub fn set_data(&mut self, data: BrowseData, cx: &mut Context<Self>) {
        self.loading = false;
        self.error = None;
        let p = data.path.clone();
        self.path_input.update(cx, |i, cx| i.set_text(p, cx));
        self.data = Some(data);
        self.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    pub fn set_error(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.loading = false;
        self.error = Some(msg.into());
        cx.notify();
    }
}

impl Render for DirBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let (path, can_confirm, n_rows) = match &self.data {
            Some(d) => (
                d.path.clone(),
                !self.require_exe || d.has_game_executable,
                d.entries.len() + usize::from(d.parent.is_some()),
            ),
            None => (String::new(), false, 0),
        };
        let bar_handle = self.scroll.0.borrow().base_handle.clone();
        let locations: Vec<BrowseLocation> = self
            .data
            .as_ref()
            .map(|d| d.roots.iter().chain(d.shortcuts.iter()).cloned().collect())
            .unwrap_or_default();

        let row = move |id: usize,
                        ic: Icon,
                        name: SharedString,
                        tag: bool,
                        target: String,
                        cx: &mut Context<Self>| {
            div()
                .id(("brow", id))
                .flex()
                .items_center()
                .gap(px(9.))
                .h(px(34.))
                .px(px(9.))
                .rounded(px(8.))
                .cursor_pointer()
                .hover(move |s| s.bg(t.panel_strong))
                .on_click(cx.listener(move |this, _, _, cx| this.navigate(target.clone(), cx)))
                .child(icon(ic).size(px(17.)).text_color(t.text))
                .child(div().flex_1().min_w_0().truncate().child(name))
                .when(tag, |d| {
                    d.child(
                        div()
                            .text_color(t.ok)
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(t.font_xs))
                            .child("BO3"),
                    )
                })
        };

        let list = if n_rows == 0 {
            let msg = if self.loading || (self.data.is_none() && self.error.is_none()) {
                "Loading..."
            } else if self.error.is_some() {
                "Folder browser unavailable"
            } else {
                "No folders found."
            };
            div()
                .m(px(12.))
                .text_color(t.text_muted)
                .child(msg)
                .into_any_element()
        } else {
            uniform_list(
                "browse-rows",
                n_rows,
                cx.processor(move |this, range: std::ops::Range<usize>, _w, cx| {
                    let Some(d) = this.data.clone() else {
                        return Vec::new();
                    };
                    let up = usize::from(d.parent.is_some());
                    range
                        .map(|ix| {
                            if up == 1 && ix == 0 {
                                row(
                                    ix,
                                    Icon::FolderOpen,
                                    "..".into(),
                                    false,
                                    d.parent.clone().unwrap_or_default(),
                                    cx,
                                )
                            } else {
                                let e = &d.entries[ix - up];
                                row(
                                    ix,
                                    Icon::FolderOpen,
                                    e.name.clone().into(),
                                    e.has_game_executable,
                                    e.path.clone(),
                                    cx,
                                )
                            }
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .size_full()
            .into_any_element()
        };

        Dialog::new("dir-browser")
            .title(self.title.clone())
            .description(self.description.clone())
            .width(px(860.))
            .on_close({
                let this = cx.entity();
                move |_, cx| this.update(cx, |_, cx| cx.emit(DirBrowserEvent::Cancel))
            })
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_1().child(self.path_input.clone()))
                    .child(Button::new("browse-go", "Go").on_click(cx.listener(
                        |this, _, _, cx| {
                            let p = this.path_input.read(cx).text().to_string();
                            this.navigate(p, cx);
                        },
                    ))),
            )
            .when_some(self.error.clone(), |d, e| {
                d.child(super::status::Alert::error(e))
            })
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .h(px(320.))
                    .child(
                        div()
                            .w(px(190.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .p(px(8.))
                            .rounded(px(10.))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.panel)
                            .child(
                                div()
                                    .px(px(6.))
                                    .pb(px(4.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(t.font_sm))
                                    .child("Locations"),
                            )
                            .children(locations.into_iter().enumerate().map(|(i, l)| {
                                let target = l.path.clone();
                                div()
                                    .id(("loc", i))
                                    .h(px(30.))
                                    .px(px(8.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(7.))
                                    .text_color(t.text_strong)
                                    .cursor_pointer()
                                    .hover(move |s| s.bg(t.hover_strong))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.navigate(target.clone(), cx)
                                    }))
                                    .child(div().truncate().child(l.label))
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.panel)
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(10.))
                                    .h(px(34.))
                                    .px(px(12.))
                                    .border_b_1()
                                    .border_color(t.border)
                                    .text_color(t.text_strong)
                                    .child(div().min_w_0().truncate().child(if path.is_empty() {
                                        "Loading...".to_string()
                                    } else {
                                        path
                                    }))
                                    .when(
                                        self.data.as_ref().is_some_and(|d| d.has_game_executable),
                                        |d| {
                                            d.child(
                                                div()
                                                    .flex_none()
                                                    .text_color(t.ok)
                                                    .font_weight(FontWeight::BOLD)
                                                    .child("BO3 detected"),
                                            )
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .relative()
                                    .flex_1()
                                    .min_h_0()
                                    .p(px(4.))
                                    .child(list)
                                    .child(Scrollbar::new("browse-bar", bar_handle)),
                            ),
                    ),
            )
            .footer(
                Button::new("browse-cancel", "Cancel")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(DirBrowserEvent::Cancel))),
            )
            .footer(
                Button::new("browse-use", self.confirm_label.clone())
                    .primary()
                    .disabled(!can_confirm)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(d) = &this.data {
                            cx.emit(DirBrowserEvent::Select(d.path.clone()));
                        }
                    })),
            )
    }
}
