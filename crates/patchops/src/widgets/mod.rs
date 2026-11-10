//! Reusable widgets. One file per widget so agents can work in parallel.
//! Stateless widgets are `RenderOnce` builders; stateful ones are `Entity<T>`
//! that emit events (subscribe with `cx.subscribe`).

pub mod button;
pub mod modal;
pub mod slider;
pub mod text_input;
pub mod toggle;

pub use button::Button;
pub use slider::{Slider, SliderEvent};
pub use text_input::{TextInput, TextInputEvent};
pub use toggle::Toggle;

use gpui::App;

/// Register key bindings for all widgets. Call once at startup.
pub fn init(cx: &mut App) {
    text_input::bind_keys(cx);
}
