//! One line of text that ends in an ellipsis when its box is narrower than the text.
//!
//! gpui's own `truncate()` / `text_ellipsis()` do not work in flex layouts here: no-wrap text keeps
//! the first (unconstrained) measure of the frame, so the text is merely clipped mid-glyph, and the
//! wrapping workaround (`line_clamp(1)`) re-uses text runs that an earlier, narrower measure already
//! shortened (the tail of the string loses its font weight). This element measures itself without
//! caching and truncates at paint time, against the bounds layout actually gave it.
//!
//! ```ignore
//! div().flex_1().min_w_0().child(ellipsis("A very long label"))
//! ```
//! The parent must let the text shrink (`min_w_0`, a `flex_1` cell, or a block with a width).

use gpui::{
    point, px, size, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, SharedString, Style, TextRun, Window,
};

const ELLIPSIS: &str = "\u{2026}";

pub struct Ellipsis {
    text: SharedString,
    right: bool,
}

/// See the module docs.
pub fn ellipsis(text: impl Into<SharedString>) -> Ellipsis {
    Ellipsis { text: text.into(), right: false }
}

impl Ellipsis {
    /// Right-align the text inside its box (a value next to its label).
    pub fn right(mut self) -> Self {
        self.right = true;
        self
    }
}

pub struct EllipsisLayout {
    font_size: Pixels,
    line_height: Pixels,
    run: TextRun,
}

fn width_of(window: &Window, text: &SharedString, font_size: Pixels, run: &TextRun) -> Pixels {
    let run = TextRun { len: text.len(), ..run.clone() };
    window.text_system().shape_line(text.clone(), font_size, &[run], None).width
}

impl IntoElement for Ellipsis {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Ellipsis {
    type RequestLayoutState = EllipsisLayout;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, EllipsisLayout) {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = style.line_height.to_pixels(font_size.into(), window.rem_size());
        let run = style.to_run(self.text.len());
        let (text, measure_run) = (self.text.clone(), run.clone());
        let mut node = Style::default();
        node.min_size.width = px(0.).into();
        let id = window.request_measured_layout(node, move |known, avail, window, _| {
            let full = width_of(window, &text, font_size, &measure_run);
            let w = known.width.unwrap_or(match avail.width {
                AvailableSpace::Definite(limit) => full.min(limit),
                _ => full,
            });
            size(w.ceil(), line_height)
        });
        (id, EllipsisLayout { font_size, line_height, run })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut EllipsisLayout,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        l: &mut EllipsisLayout,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let limit = bounds.size.width;
        let full = width_of(window, &self.text, l.font_size, &l.run);
        let shown: SharedString = if full <= limit + px(0.5) {
            self.text.clone()
        } else {
            // Longest prefix whose width plus the ellipsis still fits (binary search).
            let cuts: Vec<usize> = self.text.char_indices().map(|(i, _)| i).collect();
            let (mut lo, mut hi) = (0usize, cuts.len());
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                let cand = SharedString::from(format!("{}{ELLIPSIS}", &self.text[..cuts[mid - 1]]));
                if width_of(window, &cand, l.font_size, &l.run) <= limit {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            SharedString::from(format!("{}{ELLIPSIS}", &self.text[..if lo == 0 { 0 } else { cuts[lo - 1] }]))
        };
        let run = TextRun { len: shown.len(), ..l.run.clone() };
        let line = window.text_system().shape_line(shown, l.font_size, &[run], None);
        let dx = if self.right { (limit - line.width).max(px(0.)) } else { px(0.) };
        let dy = ((bounds.size.height - l.line_height) / 2.).max(px(0.));
        let _ = line.paint(point(bounds.left() + dx, bounds.top() + dy), l.line_height, window, cx);
    }
}
