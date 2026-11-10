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

/// Settings key set when the user dismisses the "older install found" strip.
pub const LEGACY_NOTICE_DISMISSED_KEY: &str = "legacy_install_notice_dismissed";

/// Where the strip sends the user (opened through the controller's URL action).
pub const APPS_SETTINGS_URI: &str = "ms-settings:appsfeatures";

/// Text of the "older install found" strip.
pub fn legacy_notice_text(l: &patchops_core::legacy_install::LegacyInstall) -> String {
    use patchops_core::legacy_install::Scope;
    let scope = match l.scope {
        Scope::User => "installed for this user",
        Scope::Machine => "installed for all users",
    };
    let what = l.display_name.as_deref().map_or_else(|| scope.to_string(), |n| format!("{n}, {scope}"));
    format!("An older PatchOpsIII install was found ({what}). Close PatchOpsIII, then remove the old version from Windows Settings \u{2192} Apps to avoid duplicate shortcuts.")
}

/// Detect a leftover old install unless the user already dismissed the notice. Blocking (registry
/// + settings read): call off the UI thread. `PATCHOPS_FAKE_LEGACY=user|machine` fakes a hit in debug builds.
pub fn detect_legacy_notice(core: &Core) -> Option<String> {
    use patchops_core::legacy_install::{LegacyInstall, Scope};
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("PATCHOPS_FAKE_LEGACY") {
        let scope = if v == "machine" { Scope::Machine } else { Scope::User };
        return Some(legacy_notice_text(&LegacyInstall { scope, display_name: Some("PatchOpsIII 1.3.4".into()), install_dir: None }));
    }
    if core.settings().0.get(LEGACY_NOTICE_DISMISSED_KEY).and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    patchops_core::legacy_install::detect().map(|l| legacy_notice_text(&l))
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchops_core::legacy_install::{LegacyInstall, Scope};

    #[test]
    fn notice_text() {
        let l = LegacyInstall { scope: Scope::Machine, display_name: Some("PatchOpsIII 1.3.4".into()), install_dir: None };
        assert!(legacy_notice_text(&l).contains("(PatchOpsIII 1.3.4, installed for all users)"));
        let l = LegacyInstall { scope: Scope::User, display_name: None, install_dir: None };
        assert!(legacy_notice_text(&l).contains("(installed for this user)"));
        assert!(legacy_notice_text(&l).contains("Close PatchOpsIII"));
    }
}
