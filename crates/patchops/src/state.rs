//! Small shared types between the process entry point, the controller and the RPC thread.
//!
//! The application state itself (`patchops_core::State`) lives in the
//! [`Controller`](crate::controller::Controller); there is no mutex-guarded mirror any more.

use patchops_core::log::LogEntry;
use patchops_core::Core;

/// Events sent *to* the UI from non-UI threads.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// Something outside the GUI changed core state (an RPC op ran): re-read `State`.
    Refresh,
}

/// Startup phase (legacy `BackendStatus`: `starting | ready | failed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Boot {
    Starting,
    Ready,
    Failed(String),
}

/// Subscribe to the core log; entries arrive on the returned channel (unbounded, cheap clone of
/// the entry per log call; the UI drains it in batches).
pub fn subscribe_logs(core: &Core) -> async_channel::Receiver<LogEntry> {
    let (tx, rx) = async_channel::unbounded();
    core.log.subscribe(move |e| {
        let _ = tx.try_send(e.clone());
    });
    rx
}
