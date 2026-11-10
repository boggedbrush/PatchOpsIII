//! [`Core`]: the application context every op receives (paths, log, cached settings/Steam
//! discovery). Cheap to share: wrap in `Arc<Core>`; all interior state is behind mutexes.

use std::sync::Mutex;
use std::time::SystemTime;

use crate::log::Logger;
use crate::paths::Paths;
use crate::settings::Settings;

type SteamCache = (Vec<(String, Option<SystemTime>)>, Vec<String>);

pub struct Core {
    pub paths: Paths,
    pub log: Logger,
    /// `appVersion` / `health.version`.
    pub version: String,
    settings_cache: Mutex<Option<(Option<SystemTime>, Settings)>>,
    pub(crate) steam_cache: Mutex<Option<SteamCache>>,
}

impl Core {
    /// Context for the real process environment (`HOME`, `XDG_*`, `PATCHOPSIII_VERSION_OVERRIDE`...).
    pub fn new() -> Core {
        Core::with_paths(Paths::from_env(), crate::paths::app_version())
    }

    pub fn with_paths(paths: Paths, version: String) -> Core {
        let log = Logger::new(paths.log.clone());
        Core { paths, log, version, settings_cache: Mutex::new(None), steam_cache: Mutex::new(None) }
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
