//! Single-line text input, adapted from gpui's `examples/input.rs`.
//!
//! Supports: typing (via the platform `InputHandler`, so IME/compose/dead keys work),
//! cursor, mouse + keyboard selection, backspace/delete, home/end, copy/cut/paste,
//! word-wise movement/deletion (ctrl-arrows), double-click selects a word, triple-click all,
//! password masking, read-only and disabled modes, max length, horizontal scrolling to keep
//! the cursor visible, Enter -> `Submit`, focus loss -> `Blurred`.
//! Not implemented: undo/redo, multi-line.
//!
//! ```ignore
//! let input = cx.new(|cx| TextInput::new("Enter network password", cx));
//! input.update(cx, |i, cx| { i.set_masked(true, cx); i.set_max_len(Some(20)); });
//! cx.subscribe(&input, |this, _, ev: &TextInputEvent, cx| match ev {
//!     TextInputEvent::Changed(s) => {}      // every edit
//!     TextInputEvent::Submit(s) => {}       // Enter
//!     TextInputEvent::Blurred(s) => {}      // focus left (commit-on-blur like the legacy UI)
//! }).detach();
//! ```

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, size, App, Bounds, ClipboardItem, Context,
    CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString, Style,
    Subscription, TextRun, UTF16Selection, UnderlineStyle, Window,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::ActiveTheme;

actions!(
    patchops_text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        SelectHome,
        SelectEnd,
        Paste,
        Cut,
        Copy,
        Submit,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        DeleteWordLeft,
        DeleteWordRight
    ]
);

const CONTEXT: &str = "PatchOpsTextInput";
const MASK: char = '\u{2022}';

pub fn bind_keys(cx: &mut App) {
    #[cfg(target_os = "macos")]
    let m = "cmd";
    #[cfg(not(target_os = "macos"))]
    let m = "ctrl";
    #[cfg(target_os = "macos")]
    let w = "alt";
    #[cfg(not(target_os = "macos"))]
    let w = "ctrl";
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("shift-home", SelectHome, c),
        KeyBinding::new("shift-end", SelectEnd, c),
        KeyBinding::new("enter", Submit, c),
        KeyBinding::new(&format!("{m}-a"), SelectAll, c),
        KeyBinding::new(&format!("{m}-v"), Paste, c),
        KeyBinding::new(&format!("{m}-c"), Copy, c),
        KeyBinding::new(&format!("{m}-x"), Cut, c),
        KeyBinding::new("shift-insert", Paste, c),
        KeyBinding::new(&format!("{w}-left"), WordLeft, c),
        KeyBinding::new(&format!("{w}-right"), WordRight, c),
        KeyBinding::new(&format!("{w}-shift-left"), SelectWordLeft, c),
        KeyBinding::new(&format!("{w}-shift-right"), SelectWordRight, c),
        KeyBinding::new(&format!("{w}-backspace"), DeleteWordLeft, c),
        KeyBinding::new(&format!("{w}-delete"), DeleteWordRight, c),
    ]);
}


/// UTF-16 code-unit offset -> UTF-8 byte offset in `s` (clamped to `s.len()`, rounded up to the
/// next char boundary when `offset` falls inside a surrogate pair). Always a char boundary.
fn utf16_to_byte(s: &str, offset: usize) -> usize {
    let mut byte = 0;
    let mut units = 0;
    for ch in s.chars() {
        if units >= offset {
            break;
        }
        units += ch.len_utf16();
        byte += ch.len_utf8();
    }
    byte
}

/// UTF-8 byte offset -> UTF-16 code-unit offset in `s` (a non-boundary offset rounds up).
fn byte_to_utf16(s: &str, offset: usize) -> usize {
    let mut units = 0;
    let mut bytes = 0;
    for ch in s.chars() {
        if bytes >= offset {
            break;
        }
        bytes += ch.len_utf8();
        units += ch.len_utf16();
    }
    units
}

/// New selection (absolute byte range in the content) after the IME marked `inserted`, which now
/// starts at byte `at`. The IME reports `selected_utf16` relative to the start of the inserted
/// text, in UTF-16 units; without one the cursor goes to the end of the inserted text.
fn selection_after_mark(at: usize, inserted: &str, selected_utf16: Option<&Range<usize>>) -> Range<usize> {
    match selected_utf16 {
        Some(r) => {
            let a = utf16_to_byte(inserted, r.start);
            let b = utf16_to_byte(inserted, r.end);
            at + a.min(b)..at + a.max(b)
        }
        None => at + inserted.len()..at + inserted.len(),
    }
}

/// Order a range the platform may have reported backwards.
fn ordered(r: Range<usize>) -> Range<usize> {
    if r.start <= r.end { r } else { r.end..r.start }
}

pub enum TextInputEvent {
    Changed(SharedString),
    Submit(SharedString),
    /// Focus left the input (emitted even if unchanged).
    #[allow(dead_code)] // payload not read by current owners
    Blurred(SharedString),
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
    is_selecting: bool,
    masked: bool,
    disabled: bool,
    read_only: bool,
    max_len: Option<usize>,
    char_filter: Option<fn(char) -> bool>,
    _blur_sub: Option<Subscription>,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            content: "".into(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            scroll_x: px(0.),
            is_selecting: false,
            masked: false,
            disabled: false,
            read_only: false,
            max_len: None,
            char_filter: None,
            _blur_sub: None,
        }
    }

    /// Show bullets instead of the text (password fields). Copy/cut are disabled while masked.
    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        self.masked = masked;
        cx.notify();
    }

    /// Greyed out; ignores focus and input.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    /// Selectable/copyable but not editable.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.read_only = read_only;
        cx.notify();
    }

    /// Maximum number of characters (legacy gamertag: 20).
    pub fn set_max_len(&mut self, max: Option<usize>) {
        self.max_len = max;
    }

    /// Reject typed/pasted characters for which `f` returns false (e.g. digits only).
    pub fn set_char_filter(&mut self, f: fn(char) -> bool) {
        self.char_filter = Some(f);
    }

    #[allow(dead_code)] // kit API
    pub fn set_placeholder(&mut self, p: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = p.into();
        cx.notify();
    }

    /// Update the text only when it differs (safe to call from `render` to mirror external
    /// state; does not emit and keeps the cursor when unchanged).
    pub fn sync_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.content.as_ref() != text {
            self.set_text(text.to_string(), cx);
        }
    }

    fn editable(&self) -> bool {
        !self.disabled && !self.read_only
    }

    /// Byte offset in the displayed string for a content byte offset.
    fn to_display(&self, offset: usize) -> usize {
        if self.masked {
            self.content[..offset].graphemes(true).count() * MASK.len_utf8()
        } else {
            offset
        }
    }

    /// Content byte offset for a displayed-string byte offset.
    fn display_to_content(&self, d: usize) -> usize {
        if self.masked {
            let n = d / MASK.len_utf8();
            self.content
                .grapheme_indices(true)
                .nth(n)
                .map(|(i, _)| i)
                .unwrap_or(self.content.len())
        } else {
            d
        }
    }

    fn word_range_at(&self, idx: usize) -> Range<usize> {
        for (start, seg) in self.content.split_word_bound_indices() {
            let end = start + seg.len();
            if idx >= start && idx < end {
                return start..end;
            }
        }
        self.content.len()..self.content.len()
    }

    fn prev_word(&self, offset: usize) -> usize {
        self.content
            .unicode_word_indices()
            .rev()
            .find_map(|(i, _)| (i < offset).then_some(i))
            .unwrap_or(0)
    }

    fn next_word(&self, offset: usize) -> usize {
        self.content
            .unicode_word_indices()
            .find_map(|(i, w)| (i + w.len() > offset).then_some(i + w.len()))
            .unwrap_or(self.content.len())
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.marked_range = None;
        self.debug_check();
        cx.notify();
    }

    /// Truncate `new_text` so the result respects `max_len`.
    fn clamp_insert<'a>(
        &self,
        replacing: &Range<usize>,
        new_text: &'a str,
    ) -> std::borrow::Cow<'a, str> {
        if let Some(f) = self.char_filter {
            if !new_text.chars().all(f) {
                let filtered: String = new_text.chars().filter(|c| f(*c)).collect();
                return self.clamp_len(replacing, &filtered).into_owned().into();
            }
        }
        self.clamp_len(replacing, new_text)
    }

    fn clamp_len<'a>(
        &self,
        replacing: &Range<usize>,
        new_text: &'a str,
    ) -> std::borrow::Cow<'a, str> {
        let Some(max) = self.max_len else {
            return new_text.into();
        };
        let kept = self.content.chars().count() - self.content[replacing.clone()].chars().count();
        let room = max.saturating_sub(kept);
        if new_text.chars().count() <= room {
            new_text.into()
        } else {
            new_text.chars().take(room).collect::<String>().into()
        }
    }

    /// Invariant: the selection and marked ranges lie inside the content on char boundaries
    /// (slicing anywhere else panics). Checked in debug builds only.
    fn debug_check(&self) {
        debug_assert!(
            self.selected_range.start <= self.selected_range.end
                && self.content.is_char_boundary(self.selected_range.start)
                && self.content.is_char_boundary(self.selected_range.end),
            "selection {:?} not on char boundaries of {:?}",
            self.selected_range,
            self.content
        );
        if let Some(m) = &self.marked_range {
            debug_assert!(
                m.start <= m.end && self.content.is_char_boundary(m.start) && self.content.is_char_boundary(m.end),
                "marked range {:?} not on char boundaries of {:?}",
                m,
                self.content
            );
        }
    }

    fn changed(&self, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Changed(self.content.clone()));
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.prev_word(self.cursor_offset()), cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.next_word(self.cursor_offset()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.prev_word(self.cursor_offset()), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_word(self.cursor_offset()), cx);
    }

    fn delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editable() {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.prev_word(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editable() {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.next_word(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editable() {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editable() {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        cx.emit(TextInputEvent::Submit(self.content.clone()));
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        let idx = self.index_for_mouse_position(event.position);
        if event.click_count >= 3 || (event.click_count == 2 && self.masked) {
            self.selected_range = 0..self.content.len();
            self.selection_reversed = false;
            cx.notify();
        } else if event.click_count == 2 {
            self.selected_range = self.word_range_at(idx);
            self.selection_reversed = false;
            cx.notify();
        } else if event.modifiers.shift {
            self.select_to(idx, cx);
        } else {
            self.move_to(idx, cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editable() {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace(['\n', '\r'], " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            self.copy(&Copy, window, cx);
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.debug_check();
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        self.display_to_content(
            line.closest_index_for_x(position.x - bounds.left() + self.scroll_x),
        )
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.debug_check();
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        utf16_to_byte(&self.content, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        byte_to_utf16(&self.content, offset)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editable() {
            return;
        }
        let range = ordered(
            range_utf16
                .as_ref()
                .map(|r| self.range_from_utf16(r))
                .or(self.marked_range.clone())
                .unwrap_or(self.selected_range.clone()),
        );
        let new_text = self.clamp_insert(&range, new_text);
        let new_text = new_text.as_ref();
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.selection_reversed = false;
        self.marked_range.take();
        self.debug_check();
        self.changed(cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editable() {
            return;
        }
        let range = ordered(
            range_utf16
                .as_ref()
                .map(|r| self.range_from_utf16(r))
                .or(self.marked_range.clone())
                .unwrap_or(self.selected_range.clone()),
        );
        let new_text = self.clamp_insert(&range, new_text);
        let new_text = new_text.as_ref();
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.marked_range =
            (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        // The IME's selection is relative to the inserted text (not to the whole content).
        self.selected_range = selection_after_mark(range.start, new_text, new_selected_range_utf16.as_ref());
        self.selection_reversed = false;
        self.debug_check();
        self.changed(cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(self.to_display(range.start))
                    - self.scroll_x,
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(self.to_display(range.end)) - self.scroll_x,
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let utf8_index = last_layout.index_for_x(line_point.x + self.scroll_x)?;
        Some(self.offset_to_utf16(self.display_to_content(utf8_index)))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_x: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = *cx.theme();
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let masked = input.masked && !content.is_empty();
        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), theme.text_muted.opacity(0.8))
        } else if masked {
            (
                SharedString::from(MASK.to_string().repeat(content.graphemes(true).count())),
                style.color,
            )
        } else {
            (content, style.color)
        };
        let (cursor, selected_range) = if masked {
            (
                input.to_display(cursor),
                input.to_display(selected_range.start)..input.to_display(selected_range.end),
            )
        } else {
            (cursor, selected_range)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked) = input.marked_range.as_ref().filter(|_| !masked) {
            vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        // Keep the cursor inside the visible width.
        let cursor_x = line.x_for_index(cursor);
        let mut scroll_x = input.scroll_x;
        if cursor_x - scroll_x > bounds.size.width - px(2.) {
            scroll_x = cursor_x - bounds.size.width + px(2.);
        }
        if cursor_x < scroll_x {
            scroll_x = cursor_x;
        }
        if line.width <= bounds.size.width {
            scroll_x = px(0.);
        }
        let left = bounds.left() - scroll_x;

        let (selection, cursor_quad) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(left + cursor_x, bounds.top()),
                        size(px(1.5), bounds.size.height),
                    ),
                    theme.text,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(left + line.x_for_index(selected_range.start), bounds.top()),
                        point(left + line.x_for_index(selected_range.end), bounds.bottom()),
                    ),
                    theme.selection,
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            cursor: cursor_quad,
            selection,
            scroll_x,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection)
        }
        let line = prepaint.line.take().unwrap();
        let origin = point(bounds.origin.x - prepaint.scroll_x, bounds.origin.y);
        line.paint(origin, window.line_height(), window, cx)
            .unwrap();
        if focus_handle.is_focused(window) && self.input.read(cx).editable() {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }
        let scroll_x = prepaint.scroll_x;
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.scroll_x = scroll_x;
        });
    }
}

impl gpui::Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        if self._blur_sub.is_none() {
            // Registered lazily: `new` has no `Window`. Dropped with the input (no leak).
            self._blur_sub =
                Some(
                    cx.on_focus_out(&self.focus_handle, window, |this, _ev, _w, cx| {
                        cx.emit(TextInputEvent::Blurred(this.content.clone()));
                        cx.notify();
                    }),
                );
        }
        let focused = self.focus_handle.is_focused(window);
        let disabled = self.disabled;
        div()
            .key_context(CONTEXT)
            // A disabled input must not take focus (click or Tab).
            .when(!disabled, |d| d.track_focus(&self.focus_handle))
            .cursor(if self.disabled {
                CursorStyle::OperationNotAllowed
            } else {
                CursorStyle::IBeam
            })
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .h(px(t.control_h))
            .flex()
            .items_center()
            .px(px(12.))
            .rounded(px(t.radius - 4.))
            .border_1()
            .border_color(if focused && !self.disabled {
                t.focus_ring
            } else {
                t.border
            })
            .bg(t.field)
            .text_color(t.text)
            .text_size(px(t.font_md))
            .when(self.disabled, |d| d.opacity(0.58))
            .overflow_hidden()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(TextElement { input: cx.entity() }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIXED: &str = "a\u{306B}\u{307B}b\u{1F600}c"; // a, ni, ho, b, emoji (2 UTF-16 units), c

    #[test]
    fn utf16_byte_round_trip_on_every_boundary() {
        for (byte, _) in MIXED.char_indices().chain(std::iter::once((MIXED.len(), ' '))) {
            let u = byte_to_utf16(MIXED, byte);
            assert_eq!(utf16_to_byte(MIXED, u), byte, "byte {byte} -> utf16 {u}");
            assert!(MIXED.is_char_boundary(utf16_to_byte(MIXED, u)));
        }
    }

    #[test]
    fn utf16_offsets_inside_a_surrogate_pair_round_up_to_a_boundary() {
        // units: a=0..1, ni=1..2, ho=2..3, b=3..4, emoji=4..6, c=6..7
        assert_eq!(utf16_to_byte(MIXED, 5), "a\u{306B}\u{307B}b\u{1F600}".len());
        for u in 0..12 {
            assert!(MIXED.is_char_boundary(utf16_to_byte(MIXED, u)), "utf16 offset {u}");
        }
        assert_eq!(utf16_to_byte(MIXED, 100), MIXED.len());
        assert_eq!(byte_to_utf16(MIXED, 1000), 7);
    }

    #[test]
    fn ascii_is_identity() {
        assert_eq!(utf16_to_byte("hello", 3), 3);
        assert_eq!(byte_to_utf16("hello", 3), 3);
    }

    #[test]
    #[allow(clippy::reversed_empty_ranges)]
    fn ime_selection_is_relative_to_the_inserted_text() {
        // Existing "ab", composing "にほ" at byte 2; the IME puts the cursor after the first
        // kana, i.e. UTF-16 offset 1 inside the inserted text. The old code converted this
        // against the whole content (and landed inside a 3-byte char).
        let content = format!("ab{}", "\u{306B}\u{307B}");
        let sel = selection_after_mark(2, "\u{306B}\u{307B}", Some(&(1..1)));
        assert_eq!(sel, 5..5);
        assert!(content.is_char_boundary(sel.start) && content.is_char_boundary(sel.end));
        // Whole inserted text selected.
        assert_eq!(selection_after_mark(2, "\u{306B}\u{307B}", Some(&(0..2))), 2..8);
        // No selection reported: cursor after the inserted text.
        assert_eq!(selection_after_mark(2, "\u{306B}\u{307B}", None), 8..8);
        // Emoji (2 UTF-16 units) then ASCII; offsets past the end clamp.
        assert_eq!(selection_after_mark(1, "\u{1F600}x", Some(&(2..3))), 5..6);
        assert_eq!(selection_after_mark(1, "\u{1F600}x", Some(&(0..50))), 1..6);
        // A backwards range is normalised.
        assert_eq!(selection_after_mark(0, "abc", Some(&(2..1))), 1..2);
    }

    #[test]
    #[allow(clippy::reversed_empty_ranges)]
    fn ordered_swaps_reversed_ranges() {
        assert_eq!(ordered(3..1), 1..3);
        assert_eq!(ordered(1..3), 1..3);
    }
}
