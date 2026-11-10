# GPUI 0.2.2 cheat-sheet (as used in this crate)

Everything below is lifted from code in this crate that compiles and was exercised
under Xvfb. gpui 0.2.2 is the crates.io release; docs.rs is thin, so also read the
vendored source: `~/.cargo/registry/src/*/gpui-0.2.2/` (`examples/` has `input.rs`,
`uniform_list.rs`, `window.rs`, `drag_drop.rs`, `tab_stop.rs`, ... - the best docs there are).

## Layout of this crate

| path | purpose |
|---|---|
| `src/main.rs` | arg parsing; `--rpc-stdio` runs headless (no GPUI init), else GUI |
| `src/app.rs` | `Nav` enum, `AppView` root view (sidebar + content), `run()` bootstrap, global actions |
| `src/state.rs` | `SharedState` (`Arc`, parking_lot mutex) shared by views and RPC; `UiEvent` channel to wake UI |
| `src/rpc.rs` | JSON-lines RPC: `handle_line`, `serve_stdio`, `run_headless`, `spawn_in_gui` |
| `src/theme.rs` | `Theme` global, `cx.theme()` |
| `src/widgets/*.rs` | one file per widget: button, toggle, slider, text_input, modal (+ `init` for key bindings) |
| `src/views/*.rs` | one file per sidebar tab (all demo content is PLACEHOLDER) |

To add a tab: add a variant to `Nav` (+ label), add `views/foo.rs`, add the entity field in
`AppView` and a match arm in `render`. To add a widget: new file in `widgets/`, `pub mod` it.

## Entry point

```rust
Application::new().run(move |cx: &mut App| {            // NOT App::new()
    cx.set_global(Theme::dark());                         // globals: set_global / cx.global::<T>()
    widgets::init(cx);                                    // register key bindings
    cx.bind_keys([KeyBinding::new("ctrl-q", Quit, None)]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_window_closed(|cx| if cx.windows().is_empty() { cx.quit() }).detach();
    let bounds = Bounds::centered(None, size(px(1000.), px(680.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("PatchOpsIII".into()), ..Default::default() }),
            ..Default::default()
        },
        move |_window, cx| cx.new(|cx| AppView::new(shared, ui_rx, cx)),   // returns Entity<AppView>
    ).unwrap();
});
```

## Defining a view (stateful) and rendering

```rust
pub struct TasksView { shared: Arc<SharedState>, running: bool }

impl Render for TasksView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();                       // clone small Copy-ish data out before building
        div().size_full().p_5().flex().flex_col().gap_4().bg(t.bg).text_color(t.text)
            .child("Tasks")
            .child(Button::new("run", "Run").on_click(cx.listener(|this, _ev, _window, cx| this.start(cx))))
            .when(self.running, |d| d.child("running..."))        // prelude::FluentBuilder
            .children(items.iter().map(|s| div().child(s.clone())))
    }
}
// Child views are Entities: field `dashboard: Entity<DashboardView>`, created with
//   cx.new(|cx| DashboardView::new(shared.clone(), cx))
// and embedded with `.child(self.dashboard.clone())`.
```

* Tailwind-like styling methods: `.flex() .flex_col() .gap_2() .p_4() .px_3() .w(px(180.)) .w_full() .size_full() .flex_1() .min_h_0() .rounded(px(6.)) .border_1() .border_color(c) .bg(c) .text_color(c) .text_sm() .font_family("Inter") .overflow_hidden() .absolute() .inset_0() .relative()`.
* `.id("x")` (or `.id(ElementId)`) turns a `Div` into `Stateful<Div>`, **required** for `on_click`, `hover`, `active`, scroll.
* Colours: `Hsla`; from hex: `Hsla::from(Rgba::from(rgb(0x3b82f6)))`; `c.opacity(0.5)`; `gpui::black()`.
* `px(1.)` is `Pixels`; the inner `f32` is private, use `f32::from(pixels)`.

## State updates + notify

State lives in `&mut self`; after mutating, call `cx.notify()` so the view re-renders.
Shared cross-thread state (`SharedState`) is a plain `Arc<Mutex<..>>`; views read it in `render` and
a thread that changes it must wake the UI (see "Background -> UI" below).

```rust
this.shared.with(|d| d.counter += 1);   // short lock, never hold across .await
cx.notify();
```

## Click handlers

```rust
// 1) Inside a view, when the API takes (&E, &mut Window, &mut App): cx.listener gives you `this` + Context
Button::new("inc", "+1").on_click(cx.listener(|this, _ev: &ClickEvent, _window, cx| {
    this.count += 1; cx.notify();
}))

// 2) Inside RenderOnce widgets / free functions you only have `&mut App`: capture the entity handle
let this = cx.entity();                                   // Entity<Self>
Toggle::new("t", on).on_change(move |new, _window, cx| {
    this.update(cx, |this, cx| { this.on = new; cx.notify(); })
})

// raw div: .id("x").on_click(...)  .on_mouse_down(MouseButton::Left, ...)  .on_mouse_up_out(..)
// .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, w, cx| ...))   // e.dragging() is true while button held
// cx.stop_propagation() inside a handler stops it reaching parents (used by the modal card).
```

Stateless widget pattern (Button, Toggle): `#[derive(IntoElement)] struct W {..}` + `impl RenderOnce for W { fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement }`.
Stateful widget pattern (Slider, TextInput): a struct + `impl Render`, create with `cx.new(..)`, emit with `impl EventEmitter<Ev>` + `cx.emit(Ev)`, consume with:

```rust
cx.subscribe(&slider, |this, _slider, ev: &SliderEvent, cx| { ...; cx.notify(); }).detach();
```

To push external state into a child entity during `render` without causing a notify loop, use a `set_value_silent`
style method (see `DashboardView::render`).

## Background work -> update UI

```rust
fn start(&mut self, cx: &mut Context<Self>) {
    cx.spawn(async move |this, cx| {                       // this: WeakEntity<Self>, cx: &mut AsyncApp
        let sum = cx.background_executor().spawn(async {   // runs on gpui's thread pool
            heavy_blocking_work()
        }).await;
        this.update(cx, |this, cx| {                       // back on the UI thread; Err if view was dropped
            this.running = false;
            cx.notify();
        }).ok();
    }).detach();                                           // Task is cancelled if dropped! detach() or store it
}
```

From a plain OS thread (RPC thread, file watcher, child-process reader): push into a channel and have a foreground
task `await` it (see `AppView::new` + `SharedState::attach_ui/notify_ui`; `async-channel` receivers are runtime-agnostic):

```rust
cx.spawn(async move |this, cx| {
    while let Ok(ev) = ui_rx.recv().await {
        if this.update(cx, |this, cx| { /* apply ev */ cx.notify(); }).is_err() { break; }
    }
}).detach();
```

Timers: `cx.background_executor().timer(Duration::from_millis(500)).await` inside a spawn.

## Virtualised list (`uniform_list`)

All rows must have the same height (set it explicitly). The closure only receives the visible range.

```rust
let scroll = UniformListScrollHandle::new();           // keep in the view struct, clone into the element
uniform_list("log-lines", len, cx.processor(move |this, range: Range<usize>, _window, _cx| {
        range.map(|ix| div().id(ix).h(px(18.)).child(this.line(ix))).collect::<Vec<_>>()
    }))
    .track_scroll(scroll.clone())                      // takes the handle BY VALUE
    .size_full()
// scroll.scroll_to_item(ix, ScrollStrategy::Bottom);   // programmatic scroll
```

Wrap it in a parent with a definite height (`flex_1().min_h_0()` inside a flex column), otherwise it collapses.
No scrollbar is drawn by gpui itself (TODO widget). For variable-height rows use `list()` (`ListState`) instead.

## Text input

`src/widgets/text_input.rs` (adapted from gpui `examples/input.rs`): custom `Element` that shapes text with
`window.text_system().shape_line`, registers a platform input handler via `window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx)`
so typing/IME/dead keys arrive through `EntityInputHandler::replace_text_in_range`. Key actions (backspace, arrows,
select-all, copy/cut/paste, home/end, enter) are `actions!` bound under key context `"PatchOpsTextInput"`.
Clipboard: `cx.read_from_clipboard().and_then(|i| i.text())`, `cx.write_to_clipboard(ClipboardItem::new_string(s))`.

```rust
let input = cx.new(|cx| TextInput::new("placeholder", cx));
cx.subscribe(&input, |this, _, ev: &TextInputEvent, cx| match ev {
    TextInputEvent::Changed(s) => {..}  TextInputEvent::Submit(s) => {..}
}).detach();
div().w(px(320.)).child(input.clone())
input.update(cx, |i, cx| i.set_text("abc", cx));
```
Missing: word-wise movement, double-click select word, undo/redo, multi-line, password mode.

## Focus + key bindings

```rust
actions!(patchops, [Quit, NextView]);                    // unit-struct actions
cx.bind_keys([KeyBinding::new("ctrl-tab", NextView, None),            // None = global
              KeyBinding::new("enter", Submit, Some("PatchOpsTextInput"))]);  // only inside that key_context
// view struct: focus_handle: FocusHandle = cx.focus_handle();  impl Focusable for View { fn focus_handle(..) }
div().key_context("Root").track_focus(&self.focus_handle)
     .on_action(cx.listener(|this, _: &NextView, _window, cx| this.step(1, cx)))
window.focus(&handle);                                   // inside a click handler / window.update
handle.is_focused(window)
cx.activate(true);                                       // no-op on Linux
```
Modifier names: `ctrl-`, `shift-`, `alt-`, `cmd-` (mac), `secondary-` (cmd on mac / ctrl elsewhere - not used here). Actions
dispatch to the focused element's ancestors, so the root must have a focus handle focused at startup
(`window.update(cx, |view, window, cx| window.focus(&view.focus_handle(cx)))`).

## Modal / second window

In-window modal: `widgets::modal::modal(title, body, on_close, cx)` returns a `deferred(div().absolute().inset_0().occlude()...)`
backdrop; add it as the last child of a `relative()` parent while a flag is set. Wrap the card with
`on_mouse_down(.., |_,_,cx| cx.stop_propagation())` so clicks in the card don't close it. `deferred(..).with_priority(n)` paints above siblings.

Second OS window (see `views/about.rs`):

```rust
cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(420.), px(220.)), cx))),
                               ..Default::default() },
               |_w, cx| cx.new(|_| SecondWindow)).ok();
```

## Custom element bounds

To learn an element's laid-out bounds (slider), put a `canvas(|bounds, window, cx| record(bounds), |_,_,_,_| {}).absolute().size_full()`
child in it; the prepaint closure runs with the real bounds each frame.

## Fonts

`font_family("Inter")` falls back silently if missing. Linux dev box had Inter + DejaVu Sans Mono installed; Windows and
Steam Deck will not. **TODO before release:** bundle fonts and `cx.text_system().add_fonts(vec![Cow::Borrowed(include_bytes!(..))])`.

## Build / run notes and pitfalls

* **`cargo update libc --precise 0.2.186` is baked into `Cargo.lock`.** libc >= 0.2.187 removed `ENOATTR` on Linux and
  `xattr 0.2.3` (via gpui_http_client -> zed-async-tar) stops compiling. Don't run a blanket `cargo update`.
* gpui's `gpui_http_client` / `reqwest`-less http client is a *mandatory* dep; there is no feature to drop it.
* Features of gpui 0.2.2: `x11`, `wayland`, `windows-manifest` (+ `font-kit` implied). We use `x11`+`wayland` on Linux and `windows-manifest` elsewhere (see `crates/patchops/Cargo.toml`).
* Under Xvfb **without a window manager the window stays black until the first input event** reaches it (e.g. `xdotool mousemove 600 500`). Window is mapped within ~80 ms regardless. Screenshots: `import -window root out.png`.
* Under Xvfb gpui logs `Found no xinput mouse pointers` (harmless).
* Debug gpui internals: `cargo run --features debug-log` with `RUST_LOG=info` (adds env_logger).
* `PATCHOPS_TRACE_STARTUP=1` prints time-to-first-render to stderr.
* `Task`s are cancelled when dropped; always `.detach()` or keep them in a field.
* Handlers that hold `&mut App` only cannot `cx.notify()` a view; capture `cx.entity()` and `.update()`.
* `track_scroll` takes the scroll handle by value (clone it).
* RPC: `PATCHOPS_BENCH_RPC=1 patchops` runs the stdin/stdout JSON loop on a thread inside the GUI; `patchops --rpc-stdio` is the headless mode (~2 ms to first reply, ~3.7 MB PSS). Ops: `health echo get_state click set_toggle set_slider push_log navigate`.

## Windows notes (cannot build here)

* gpui 0.2.2 has a real Windows backend (`src/platform/windows`: Direct3D 11 + DirectWrite, no Vulkan).
* Its `build.rs` needs **`fxc.exe`** (Windows SDK; override with `GPUI_FXC_PATH`) to compile HLSL, and `embed-resource` (needs `rc.exe`/windres) for the `windows-manifest` feature. Build natively on Windows (MSVC toolchain + Windows SDK); cross-compiling from Linux is unlikely to work without extra effort.
* Nothing in our own deps is Linux-only; `parking_lot`, `async-channel`, `serde_json`, `unicode-segmentation` are portable. Keep Linux-only code behind `cfg(target_os = "linux")`.
* Key bindings: use `ctrl-` on Windows/Linux (done via `cfg(target_os = "macos")` in `text_input::bind_keys`).
* Steam Deck: Vulkan (RADV) via blade-graphics; X11 and Wayland both enabled so gamescope/XWayland and native Wayland work.
