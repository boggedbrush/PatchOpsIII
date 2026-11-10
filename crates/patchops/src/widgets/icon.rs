//! Icons: the legacy UI uses `lucide-react`; the same glyphs (lucide-static 0.468, ISC licence)
//! are embedded as tiny SVGs (see `assets/icons/`) and drawn with gpui's `svg()` element
//! (tinted with the current text colour).
//!
//! ```ignore
//! icon(Icon::FolderOpen).size(px(16.)).text_color(t.text_muted)
//! ```
//! Use [`Spinner`](super::progress::Spinner) for an animated `LoaderCircle`.

use gpui::{px, svg, Styled, Svg};

macro_rules! icons {
    ($($variant:ident => $file:literal),+ $(,)?) => {
        /// Embedded icon set. Add a glyph: drop the SVG in `assets/icons/` and add a line here.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[allow(dead_code)] // the full lucide subset is kept; not every glyph is used yet
        pub enum Icon { $($variant),+ }

        impl Icon {
            /// Asset-source path.
            pub const fn path(self) -> &'static str {
                match self { $(Icon::$variant => concat!("icons/", $file, ".svg")),+ }
            }
        }

        /// Every embedded icon `(path, bytes)`; consumed by [`super::assets::Assets`].
        pub(super) static ICON_BYTES: &[(&str, &[u8])] = &[
            $((concat!("icons/", $file, ".svg"),
               include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/", $file, ".svg")) as &[u8])),+
        ];
    };
}

icons! {
    ArrowUp => "arrow-up",
    Check => "check",
    ChevronDown => "chevron-down",
    ChevronRight => "chevron-right",
    ChevronUp => "chevron-up",
    CircleCheck => "circle-check",
    CircleX => "circle-x",
    Clipboard => "clipboard",
    Copy => "copy",
    Cpu => "cpu",
    Download => "download",
    Eraser => "eraser",
    ExternalLink => "external-link",
    EyeOff => "eye-off",
    Eye => "eye",
    FolderOpen => "folder-open",
    Folder => "folder",
    Gem => "gem",
    HardDrive => "hard-drive",
    House => "house",
    Image => "image",
    Info => "info",
    KeyRound => "key-round",
    LayoutDashboard => "layout-dashboard",
    LoaderCircle => "loader-circle",
    Minus => "minus",
    Plus => "plus",
    RefreshCw => "refresh-cw",
    RotateCcw => "rotate-ccw",
    Save => "save",
    Search => "search",
    Settings => "settings",
    ShieldCheck => "shield-check",
    Square => "square",
    SquarePlay => "square-play",
    Trash2 => "trash-2",
    TriangleAlert => "triangle-alert",
    UserRound => "user-round",
    Wrench => "wrench",
    X => "x",
}

/// A 16px svg element for `icon`; style it with `.size(..)` / `.text_color(..)`.
pub fn icon(icon: Icon) -> Svg {
    svg().path(icon.path()).size(px(16.)).flex_none()
}
