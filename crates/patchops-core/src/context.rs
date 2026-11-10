//! [`Core`]: the application context every op receives (paths, log, cached settings/Steam
//! discovery). Cheap to share: wrap in `Arc<Core>`; all interior state is behind mutexes.

use std::sync::{Condvar, Mutex};
use std::thread::ThreadId;
use std::time::SystemTime;

use crate::log::Logger;
use crate::paths::Paths;
use crate::settings::Settings;

type SteamCache = (Vec<(String, Option<SystemTime>)>, Vec<String>);

/// Owner + recursion depth of the operation lock.
#[derive(Default)]
struct OpLockState {
    owner: Option<ThreadId>,
    depth: u32,
}

/// Guard of [`Core::lock_ops`]; releases one level of the (re-entrant) operation lock on drop.
#[must_use = "the operation lock is released when the guard is dropped"]
pub struct OpGuard<'a> {
    core: &'a Core,
}

impl Drop for OpGuard<'_> {
    fn drop(&mut self) {
        let mut st = self.core.op_lock.lock().unwrap_or_else(|e| e.into_inner());
        st.depth -= 1;
        if st.depth == 0 {
            st.owner = None;
            drop(st);
            self.core.op_cv.notify_one();
        }
    }
}

pub struct Core {
    pub paths: Paths,
    pub log: Logger,
    /// `appVersion` / `health.version`.
    pub version: String,
    settings_cache: Mutex<Option<(Option<SystemTime>, Settings)>>,
    pub(crate) steam_cache: Mutex<Option<SteamCache>>,
    op_lock: Mutex<OpLockState>,
    op_cv: Condvar,
}

impl Core {
    /// Context for the real process environment (`HOME`, `XDG_*`, `PATCHOPSIII_VERSION_OVERRIDE`...).
    pub fn new() -> Core {
        Core::with_paths(Paths::from_env(), crate::paths::app_version())
    }

    /// Creating a `Core` never touches the log file (no start-up trim): the GUI calls
    /// `core.log.trim_if_needed()` off the main thread, the elevated CLI child never trims.
    pub fn with_paths(paths: Paths, version: String) -> Core {
        let log = Logger::new(paths.log.clone());
        Core { paths, log, version, settings_cache: Mutex::new(None),
            steam_cache: Mutex::new(None),
            op_lock: Mutex::new(OpLockState::default()),
            op_cv: Condvar::new(),
        }
    }

    /// The operation lock: every typed op that changes anything on disk (game files, config,
    /// Steam files, settings, the Enhanced state file) holds it for its whole duration, so two
    /// mutating ops can never interleave (e.g. the GUI's depot watcher swapping the exe while the
    /// user restores the current build). It is **re-entrant per thread**: a typed op that calls
    /// another typed op (`reset_stock` -> uninstall helpers, `workshop_install` -> launch
    /// options) simply nests, so every public mutating entry point can take it without
    /// deadlocking. Other threads block until the outermost guard is dropped.
    ///
    /// Read-only ops (`status`, `browse`, `health`, `logs/payload`) never take it, so they do not
    /// wait behind a long download; the one write `status` can make is skipped while an op is in
    /// flight ([`Core::try_lock_ops`]).
    pub fn lock_ops(&self) -> OpGuard<'_> {
        let me = std::thread::current().id();
        let mut st = self.op_lock.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match st.owner {
                None => break,
                Some(o) if o == me => break,
                Some(_) => st = self.op_cv.wait(st).unwrap_or_else(|e| e.into_inner()),
            }
        }
        st.owner = Some(me);
        st.depth += 1;
        OpGuard { core: self }
    }

    /// [`Core::lock_ops`] without waiting: `None` when another thread's op is running.
    pub fn try_lock_ops(&self) -> Option<OpGuard<'_>> {
        let me = std::thread::current().id();
        let mut st = self.op_lock.lock().unwrap_or_else(|e| e.into_inner());
        match st.owner {
            None => {}
            Some(o) if o == me => {}
            Some(_) => return None,
        }
        st.owner = Some(me);
        st.depth += 1;
        Some(OpGuard { core: self })
    }

    /// Current settings (re-read when the file's mtime changes).
    pub fn settings(&self) -> Settings {
        let mtime = std::fs::metadata(&self.paths.settings).and_then(|m| m.modified()).ok();
        let mut cache = self.settings_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((m, s)) = cache.as_ref() {
            if *m == mtime {
                return s.clone();
            }
        }
        let s = Settings::load(&self.paths.settings);
        *cache = Some((mtime, s.clone()));
        s
    }

    /// Read-modify-write the settings file.
    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> std::io::Result<()> {
        let _op = self.lock_ops();
        let mut s = self.settings();
        f(&mut s);
        s.save(&self.paths.settings)?;
        let mtime = std::fs::metadata(&self.paths.settings).and_then(|m| m.modified()).ok();
        *self.settings_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some((mtime, s));
        Ok(())
    }

    /// Resolved game directory (settings, Steam libraries, platform defaults), see [`crate::game`].
    pub fn game_dir(&self) -> Option<String> {
        crate::game::find_game_dir(self)
    }
}

impl Default for Core {
    fn default() -> Self {
        Core::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn core() -> Arc<Core> {
        Arc::new(Core::with_paths(Paths::with_app_data(std::env::temp_dir().join("patchops-ctx-home"), std::env::temp_dir().join("patchops-ctx-app")), "t".into()))
    }

    #[test]
    fn reentrant_on_one_thread() {
        let c = core();
        let a = c.lock_ops();
        let b = c.lock_ops();
        assert!(c.try_lock_ops().is_some());
        drop(b);
        drop(a);
        // Fully released: another thread can take it.
        let c2 = c.clone();
        assert!(std::thread::spawn(move || c2.try_lock_ops().is_some()).join().unwrap());
    }

    #[test]
    fn other_threads_wait_for_the_outermost_guard() {
        let c = core();
        let inside = Arc::new(AtomicU32::new(0));
        let max_inside = Arc::new(AtomicU32::new(0));
        let hs: Vec<_> = (0..8)
            .map(|_| {
                let (c, inside, max_inside) = (c.clone(), inside.clone(), max_inside.clone());
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        let _outer = c.lock_ops();
                        let _nested = c.lock_ops(); // nested op inside an op must not deadlock
                        let n = inside.fetch_add(1, Ordering::SeqCst) + 1;
                        max_inside.fetch_max(n, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_micros(200));
                        inside.fetch_sub(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        assert_eq!(max_inside.load(Ordering::SeqCst), 1, "two ops ran concurrently");
    }

    #[test]
    fn try_lock_fails_while_another_thread_holds_it_and_panics_release() {
        let c = core();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let (tx2, rx2) = std::sync::mpsc::channel::<()>();
        let c2 = c.clone();
        let h = std::thread::spawn(move || {
            let _g = c2.lock_ops();
            tx.send(()).unwrap();
            rx2.recv().unwrap();
            panic!("op panicked while holding the lock");
        });
        rx.recv().unwrap();
        assert!(c.try_lock_ops().is_none());
        tx2.send(()).unwrap();
        assert!(h.join().is_err());
        // The guard was dropped during unwinding.
        assert!(c.try_lock_ops().is_some());
    }

    #[test]
    fn typed_mutating_ops_wait_for_the_lock_but_status_does_not() {
        let root = std::env::temp_dir().join(format!("patchops-ctx-ops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        let c = Arc::new(Core::with_paths(Paths::with_app_data(root.join("home"), root.join("app")), "t".into()));
        let held = c.lock_ops(); // an op (e.g. a long download) is in flight on this thread
        let done = Arc::new(AtomicU32::new(0));
        let (c2, d2) = (c.clone(), done.clone());
        let h = std::thread::spawn(move || {
            crate::ops::basics::set_release_channel(&c2, "beta").unwrap();
            d2.store(1, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(done.load(Ordering::SeqCst), 0, "mutating op must wait for the lock");
        // A read-only op on another thread is not blocked.
        let c3 = c.clone();
        let st = std::thread::spawn(move || crate::ops::basics::status(&c3).release_channel).join().unwrap();
        assert_eq!(st, "stable");
        drop(held);
        h.join().unwrap();
        assert_eq!(done.load(Ordering::SeqCst), 1);
        assert_eq!(c.settings().release_channel(), "beta");
        let _ = std::fs::remove_dir_all(&root);
    }
}
