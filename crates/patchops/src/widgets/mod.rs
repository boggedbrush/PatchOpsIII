//! Reusable widget kit. One file per widget family. Stateless widgets are `RenderOnce`
//! builders; stateful ones (`TextInput`, `Slider`, `Select`, `NumberInput`, `LogList`,
//! `DirBrowser`, `ToastHost`) are `Entity<T>` that emit events (subscribe with `cx.subscribe`).
//! See `GPUI_NOTES.md` ("Widget kit") for one snippet per widget.

pub mod assets;
pub mod button;
pub mod dir_browser;
pub mod ellipsis;
pub mod form;
pub mod icon;
pub mod log_list;
pub mod modal;
pub mod nav;
pub mod number_input;
pub mod panel;
pub mod progress;
pub mod scroll;
pub mod select;
pub mod slider;
pub mod status;
pub mod tabs;
pub mod text_input;
pub mod toast;
pub mod toggle;
pub mod tooltip;

pub use assets::Assets;
#[allow(unused_imports)] // kit surface (gallery / views)
pub use button::{Button, ButtonKind, IconButton};
pub use ellipsis::ellipsis;
pub use dir_browser::{BrowseData, BrowseEntry, BrowseLocation, DirBrowser, DirBrowserEvent};
#[allow(unused_imports)]
pub use form::{Field, FormRow, SettingRow};
pub use icon::{icon, Icon};
#[allow(unused_imports)]
pub use log_list::{LogLevel, LogList};
pub use modal::{confirm_dialog, Dialog};
pub use nav::NavItem;
pub use number_input::{NumberInput, NumberInputEvent};
#[allow(unused_imports)]
pub use panel::{divider, Card, Collapsible, DropZone, Group, Panel};
pub use progress::{ProgressBar, Spinner};
#[allow(unused_imports)]
pub use scroll::{ScrollArea, Scrollbar};
#[allow(unused_imports)]
pub use select::{Select, SelectEvent, SelectOption};
pub use slider::{Slider, SliderEvent};
#[allow(unused_imports)]
pub use status::{requirement, Alert, Badge, Chip, Metric, StatusPill, StatusRow, Tone};
pub use tabs::{Segmented, SubTabs};
pub use text_input::{TextInput, TextInputEvent};
pub use toast::{ToastHost, ToastKind};
pub use toggle::{Checkbox, Radio, Toggle};
pub use tooltip::tooltip;

use gpui::{actions, App, ClickEvent, InteractiveElement, KeyBinding, Window};


/// Boxed callback shapes shared by the widgets.
pub type ClickFn = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
pub type BoolFn = Box<dyn Fn(bool, &mut Window, &mut App)>;
pub type ActionFn = Box<dyn Fn(&mut Window, &mut App)>;
pub type PathsFn = Box<dyn Fn(&[std::path::PathBuf], &mut Window, &mut App)>;

actions!(patchops_widgets, [FocusNext, FocusPrev]);

/// Register key bindings and fonts for all widgets. Call once at startup.
pub fn init(cx: &mut App) {
    assets::register_fonts(cx);
    text_input::bind_keys(cx);
    slider::bind_keys(cx);
    select::bind_keys(cx);
    number_input::bind_keys(cx);
    log_list::bind_keys(cx);
    cx.bind_keys([KeyBinding::new("tab", FocusNext, None), KeyBinding::new("shift-tab", FocusPrev, None)]);
}

/// Make Tab / Shift-Tab move focus between the widgets' tab stops. Call on the root element:
/// `div().key_context("Root").tab_nav()`.
pub trait TabNav: InteractiveElement + Sized {
    fn tab_nav(self) -> Self {
        self.on_action(|_: &FocusNext, window: &mut Window, cx| step_focus(window, cx, true))
            .on_action(|_: &FocusPrev, window: &mut Window, cx| step_focus(window, cx, false))
    }
}

impl<T: InteractiveElement + Sized> TabNav for T {}

/// Move to the next/previous tab stop. When focus sits on a non-tab-stop (the app root right
/// after startup) start from the first/last tab stop instead of doing nothing.
fn step_focus(window: &mut Window, cx: &mut App, forward: bool) {
    let cur = window.focused(cx);
    if cur.as_ref().is_some_and(|c| !c.tab_stop) {
        window.blur();
    }
    if forward { window.focus_next() } else { window.focus_prev() }
    if window.focused(cx).is_none() {
        if let Some(c) = cur {
            window.focus(&c);
        }
    }
}
