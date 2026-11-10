//! Numeric input with stepper arrows (legacy `NumberRow`): type a number, Up/Down keys or the
//! arrows step, value is clamped to `min..=max` and committed on Enter / blur / step.

use gpui::{
    actions, div, prelude::*, px, App, Context, Entity, EventEmitter, FocusHandle, Focusable,
    KeyBinding, Render, Subscription, Window,
};

use super::icon::{icon, Icon};
use super::text_input::{TextInput, TextInputEvent};
use crate::theme::ActiveTheme;

actions!(patchops_number, [Increment, Decrement]);
const CONTEXT: &str = "PatchOpsNumberInput";

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", Increment, c),
        KeyBinding::new("down", Decrement, c),
    ]);
}

pub enum NumberInputEvent {
    /// A valid number was typed (not yet clamped/committed) or stepped.
    Changed(f64),
    /// Clamped value after Enter / blur / step: persist this one.
    Committed(f64),
}

/// ```ignore
/// let refresh = cx.new(|cx| NumberInput::new(165., 1., 240., cx));     // value, min, max
/// refresh.update(cx, |n, cx| n.set_step(1.).set_disabled(false, cx));
/// cx.subscribe(&refresh, |this, _, ev: &NumberInputEvent, cx| {
///     if let NumberInputEvent::Committed(v) = ev { this.set_refresh(*v as u32, cx) }
/// }).detach();
/// refresh.update(cx, |n, cx| n.set_value(state.refresh as f64, cx));   // mirror external state
/// ```
pub struct NumberInput {
    input: Entity<TextInput>,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    disabled: bool,
    /// The user has typed since the last commit; external `set_value` is ignored meanwhile.
    editing: bool,
    _sub: Subscription,
}

impl EventEmitter<NumberInputEvent> for NumberInput {}

impl Focusable for NumberInput {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

fn fmt(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

impl NumberInput {
    pub fn new(value: f64, min: f64, max: f64, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            let mut i = TextInput::new("", cx);
            i.set_char_filter(|c| c.is_ascii_digit() || c == '-' || c == '.');
            i.set_text(fmt(value), cx);
            i
        });
        let sub = cx.subscribe(&input, |this, _, ev: &TextInputEvent, cx| match ev {
            TextInputEvent::Changed(s) => {
                this.editing = true;
                if let Ok(v) = s.parse::<f64>() {
                    cx.emit(NumberInputEvent::Changed(v));
                }
            }
            TextInputEvent::Submit(_) | TextInputEvent::Blurred(_) => this.commit(cx),
        });
        Self {
            input,
            value,
            min,
            max,
            step: 1.0,
            disabled: false,
            editing: false,
            _sub: sub,
        }
    }

    pub fn set_step(&mut self, step: f64) -> &mut Self {
        self.step = step;
        self
    }

    pub fn set_range(&mut self, min: f64, max: f64) {
        self.min = min;
        self.max = max;
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        self.input.update(cx, |i, cx| i.set_disabled(disabled, cx));
        cx.notify();
    }

    /// Mirror external state (no event). Ignored while the user is typing in it.
    pub fn set_value(&mut self, v: f64, cx: &mut Context<Self>) {
        if (v - self.value).abs() > f64::EPSILON && !self.editing {
            self.value = v;
            self.input.update(cx, |i, cx| i.set_text(fmt(v), cx));
        }
    }

    /// Commit what is typed right now (if the user is still editing) and return the clamped value.
    /// Button handlers call this first: a same-frame click can arrive before the blur commit has
    /// reached the owner, so they must not rely on a value captured at render time.
    pub fn commit_if_editing(&mut self, cx: &mut Context<Self>) -> f64 {
        if self.editing {
            self.commit(cx);
        }
        self.value
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        let parsed = self
            .input
            .read(cx)
            .text()
            .parse::<f64>()
            .unwrap_or(self.value);
        let v = parsed.clamp(self.min, self.max);
        self.value = v;
        self.editing = false;
        self.input.update(cx, |i, cx| i.set_text(fmt(v), cx));
        cx.emit(NumberInputEvent::Committed(v));
    }

    fn step_by(&mut self, dir: f64, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let cur = self
            .input
            .read(cx)
            .text()
            .parse::<f64>()
            .unwrap_or(self.value);
        let v = (cur + dir * self.step).clamp(self.min, self.max);
        self.value = v;
        self.input.update(cx, |i, cx| i.set_text(fmt(v), cx));
        cx.emit(NumberInputEvent::Changed(v));
        cx.emit(NumberInputEvent::Committed(v));
    }
}

impl Render for NumberInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let arrow = |id: &'static str, ic: Icon, dir: f64, cx: &mut Context<Self>| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .w(px(22.))
                .h(px(t.control_h / 2. - 1.))
                .cursor_pointer()
                .hover(move |s| s.bg(t.hover_strong))
                .child(icon(ic).size(px(12.)).text_color(t.text_muted))
                .on_click(cx.listener(move |this, _, _, cx| this.step_by(dir, cx)))
        };
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &Increment, _, cx| this.step_by(1., cx)))
            .on_action(cx.listener(|this, _: &Decrement, _, cx| this.step_by(-1., cx)))
            .relative()
            .w_full()
            .child(self.input.clone())
            .when(!self.disabled, |d| {
                d.child(
                    div()
                        .absolute()
                        .right(px(2.))
                        .top(px(1.))
                        .flex()
                        .flex_col()
                        .child(arrow("num-up", Icon::ChevronUp, 1., cx))
                        .child(arrow("num-down", Icon::ChevronDown, -1., cx)),
                )
            })
    }
}
