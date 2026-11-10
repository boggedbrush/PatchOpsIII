//! Scroll container with a visible, draggable scrollbar (gpui draws none by itself).
//!
//! ```ignore
//! ScrollArea::new("content").child(big_column)          // handles its own ScrollHandle
//! // or attach a scrollbar to your own scrollable / uniform_list:
//! div().relative().child(uniform_list(..).track_scroll(h.clone())).child(Scrollbar::new("sb", h.0.borrow().base_handle.clone()))
//! ```

use gpui::{
    div, fill, point, prelude::*, px, relative, size, AnyElement, App, Bounds, Element, ElementId,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement, IntoElement,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, ScrollHandle, StatefulInteractiveElement, Style, Window,
};

use crate::theme::ActiveTheme;

const BAR_W: f32 = 8.0;
const MIN_THUMB: f32 = 28.0;

#[derive(Default)]
struct DragState {
    /// Mouse y offset inside the thumb at drag start.
    grab: Option<Pixels>,
    hovered: bool,
}

/// Vertical scrollbar overlay for any element tracked by `handle`. Place it as an absolutely
/// positioned child of a `relative()` parent (see [`ScrollArea`], which does this for you).
pub struct Scrollbar {
    id: ElementId,
    handle: ScrollHandle,
}

impl Scrollbar {
    pub fn new(id: impl Into<ElementId>, handle: ScrollHandle) -> Self {
        Self {
            id: id.into(),
            handle,
        }
    }
}

pub struct ScrollbarPrepaint {
    hitbox: Option<Hitbox>,
    thumb: Bounds<Pixels>,
    track: Bounds<Pixels>,
}

impl IntoElement for Scrollbar {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Scrollbar {
    type RequestLayoutState = ();
    type PrepaintState = ScrollbarPrepaint;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style {
            position: gpui::Position::Absolute,
            ..Style::default()
        };
        style.inset.top = px(0.).into();
        style.inset.bottom = px(0.).into();
        style.inset.right = px(0.).into();
        style.size.width = px(BAR_W + 4.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> ScrollbarPrepaint {
        let max = self.handle.max_offset().height;
        let view_h = bounds.size.height;
        let track = Bounds::new(
            point(bounds.right() - px(BAR_W + 2.), bounds.top() + px(2.)),
            size(px(BAR_W), (view_h - px(4.)).max(px(0.))),
        );
        if max <= px(1.) || track.size.height <= px(MIN_THUMB) {
            return ScrollbarPrepaint {
                hitbox: None,
                thumb: Bounds::default(),
                track,
            };
        }
        let content_h = view_h + max;
        let th = (track.size.height * (view_h / content_h)).max(px(MIN_THUMB));
        let travel = track.size.height - th;
        let frac = (-self.handle.offset().y / max).clamp(0., 1.);
        let thumb = Bounds::new(
            point(track.left(), track.top() + travel * frac),
            size(px(BAR_W), th),
        );
        let hitbox = Some(window.insert_hitbox(track, HitboxBehavior::Normal));
        ScrollbarPrepaint {
            hitbox,
            thumb,
            track,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        pre: &mut ScrollbarPrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(hitbox) = pre.hitbox.clone() else {
            return;
        };
        let t = *cx.theme();
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| DragState::default());
        let (dragging, hovered) = {
            let s = state.read(cx);
            (s.grab.is_some(), s.hovered)
        };
        let (track, thumb) = (pre.track, pre.thumb);
        window.paint_quad(fill(track, t.scroll_track).corner_radii(px(BAR_W / 2.)));
        let color = if dragging || hovered {
            t.scroll_thumb_hover
        } else {
            t.scroll_thumb
        };
        window.paint_quad(fill(thumb, color).corner_radii(px(BAR_W / 2.)));

        let handle = self.handle.clone();
        // Press: grab the thumb, or jump so the thumb centres on the click.
        window.on_mouse_event({
            let (state, handle, hitbox) = (state.clone(), handle.clone(), hitbox.clone());
            move |ev: &MouseDownEvent, phase, window, cx| {
                if !phase.bubble() || ev.button != MouseButton::Left || !hitbox.is_hovered(window) {
                    return;
                }
                let grab = if thumb.contains(&ev.position) {
                    ev.position.y - thumb.top()
                } else {
                    thumb.size.height / 2.
                };
                state.update(cx, |s, _| s.grab = Some(grab));
                scroll_to(&handle, track, thumb.size.height, ev.position.y - grab);
                window.refresh();
                cx.stop_propagation();
            }
        });
        window.on_mouse_event({
            let (state, handle) = (state.clone(), handle.clone());
            move |ev: &MouseMoveEvent, _, window, cx| {
                let (grab, was_hover) = {
                    let s = state.read(cx);
                    (s.grab, s.hovered)
                };
                if let Some(grab) = grab {
                    if ev.dragging() {
                        scroll_to(&handle, track, thumb.size.height, ev.position.y - grab);
                        window.refresh();
                    }
                } else {
                    let h = hitbox.is_hovered(window);
                    if h != was_hover {
                        state.update(cx, |s, _| s.hovered = h);
                        window.refresh();
                    }
                }
            }
        });
        window.on_mouse_event(move |_: &MouseUpEvent, _, window, cx| {
            if state.read(cx).grab.is_some() {
                state.update(cx, |s, _| s.grab = None);
                window.refresh();
            }
        });
    }
}

/// Scroll so the thumb's top edge sits at window y `thumb_top`.
fn scroll_to(handle: &ScrollHandle, track: Bounds<Pixels>, thumb_h: Pixels, thumb_top: Pixels) {
    let travel = (track.size.height - thumb_h).max(px(1.));
    let frac = ((thumb_top - track.top()) / travel).clamp(0., 1.);
    let max = handle.max_offset().height;
    let off = handle.offset();
    handle.set_offset(Point {
        x: off.x,
        y: -(max * frac),
    });
}

/// Vertically scrolling container with a scrollbar. Children stack in a column.
///
/// ```ignore
/// ScrollArea::new("tools").gap(px(12.)).child(card_a).child(card_b)
/// ```
/// Give it a bounded height (`flex_1().min_h_0()` inside a flex column or an explicit `.h()`).
#[derive(IntoElement)]
pub struct ScrollArea {
    id: ElementId,
    handle: Option<ScrollHandle>,
    gap: Pixels,
    children: Vec<AnyElement>,
}

impl ScrollArea {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            handle: None,
            gap: px(0.),
            children: Vec::new(),
        }
    }
    /// Use your own handle (e.g. to scroll programmatically).
    #[allow(dead_code)] // kit API
    pub fn handle(mut self, h: &ScrollHandle) -> Self {
        self.handle = Some(h.clone());
        self
    }
    /// Vertical gap between children.
    pub fn gap(mut self, g: Pixels) -> Self {
        self.gap = g;
        self
    }
}

impl ParentElement for ScrollArea {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for ScrollArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let handle = self.handle.unwrap_or_else(|| {
            window
                .use_keyed_state("scroll-handle", cx, |_, _| ScrollHandle::new())
                .read(cx)
                .clone()
        });
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                div()
                    .id(self.id.clone())
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap(self.gap)
                    .pr(px(BAR_W + 6.))
                    .overflow_y_scroll()
                    .track_scroll(&handle)
                    .children(self.children),
            )
            .child(Scrollbar::new("scroll-bar", handle))
    }
}
