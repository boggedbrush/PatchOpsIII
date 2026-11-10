//! Shared application state.
//!
//! `SharedState` is the single source of truth that both the GUI views and the
//! JSON-lines RPC loop (headless or in-process) operate on. It contains no GUI
//! types, so it works in `--rpc-stdio` mode where GPUI is never initialised.
//! Views read it in `render`, mutate it from event handlers, and call
//! `notify_ui` (or are woken by the RPC thread) so the root view re-renders.

use std::sync::Arc;

use parking_lot::Mutex;

use patchops_core::Core;

/// Events sent *to* the UI from non-UI threads (RPC thread, workers).
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// Shared state changed; re-render.
    Refresh,
    /// Switch the sidebar to the nav entry with this index.
    Navigate(usize),
}

#[derive(Debug, Clone, Default)]
pub struct Data {
    pub counter: u64,
    pub toggle: bool,
    pub slider: f32,
    pub text: String,
    pub log: Vec<String>,
}

#[derive(Default)]
pub struct SharedState {
    /// The application core (settings, log, ops). Views call typed `patchops_core::ops::*`
    /// functions on it directly; the RPC thread serves the same instance.
    pub core: Arc<Core>,
    data: Mutex<Data>,
    ui_tx: Mutex<Option<async_channel::Sender<UiEvent>>>,
}

impl SharedState {
    pub fn new() -> Self {
        let mut data = Data {
            slider: 0.4,
            ..Data::default()
        };
        // Placeholder demo content.
        data.log = (0..1000)
            .map(|i| format!("[{:05}] placeholder log line {i} - the quick brown fox jumps over the lazy dog", i))
            .collect();
        Self {
            core: Arc::new(Core::new()),
            data: Mutex::new(data),
            ui_tx: Mutex::new(None),
        }
    }

    /// Run a closure with the data locked. Keep it short; never hold across `.await`.
    pub fn with<R>(&self, f: impl FnOnce(&mut Data) -> R) -> R {
        f(&mut self.data.lock())
    }

    pub fn snapshot(&self) -> Data {
        self.data.lock().clone()
    }

    pub fn log_len(&self) -> usize {
        self.data.lock().log.len()
    }

    pub fn push_log(&self, line: impl Into<String>) {
        self.data.lock().log.push(line.into());
        self.notify_ui(UiEvent::Refresh);
    }

    /// Connect the UI; returns the receiving end. Called once by the root view.
    pub fn attach_ui(&self) -> async_channel::Receiver<UiEvent> {
        let (tx, rx) = async_channel::unbounded();
        *self.ui_tx.lock() = Some(tx);
        rx
    }

    /// Wake the UI from any thread. No-op in headless mode.
    pub fn notify_ui(&self, ev: UiEvent) {
        if let Some(tx) = self.ui_tx.lock().as_ref() {
            let _ = tx.try_send(ev);
        }
    }

    pub fn is_gui(&self) -> bool {
        self.ui_tx.lock().is_some()
    }
}

impl SharedState {
    /// Copy out a window of log lines (cheap: only the visible range).
    pub fn snapshot_range(&self, range: std::ops::Range<usize>) -> Vec<String> {
        let d = self.data.lock();
        let end = range.end.min(d.log.len());
        let start = range.start.min(end);
        d.log[start..end].to_vec()
    }
}
