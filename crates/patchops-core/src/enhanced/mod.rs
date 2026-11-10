//! Local (non-network, non-Steam) machinery of the BO3 Enhanced / executable-swap group.
//!
//! - `zip`: minimal zip reader/extractor (dump archives, Enhanced archive)
//! - `hashcache`: cached SHA-256 of the game executable
//! - `state_file`: `bo3_enhanced_state.json` (legacy `bo3_enhanced.py` state)
//! - `files`: dump validation, file placement / removal (legacy `bo3_enhanced.py`)
//! - `exe`: executable integrity, preservation and swapping (legacy `backend/api.py`)
//! - `linux`: Proton tool, `CompatToolMapping` and launch-option bookkeeping
//!
//! The op layer and the `enhanced`/`exeSwap` state builders live in [`crate::ops::enhanced`].

pub mod exe;
pub mod files;
pub mod hashcache;
pub mod linux;
pub mod state_file;
pub mod zip;

#[cfg(test)]
pub(crate) mod testkit {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::context::Core;
    use crate::paths::Paths;

    /// Isolated sandbox: `<tmp>/home` (Steam root at `.steam/steam`), app data, and a game dir.
    pub struct Sandbox {
        pub root: PathBuf,
        pub core: Core,
        pub game: PathBuf,
        pub steam: PathBuf,
    }

    impl Sandbox {
        pub fn new() -> Sandbox {
            static N: AtomicU32 = AtomicU32::new(0);
            let root = std::env::temp_dir().join(format!("patchops-enh-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&root);
            let home = root.join("home");
            let steam = home.join(".steam/steam");
            let game = steam.join("steamapps/common/Call of Duty Black Ops III");
            std::fs::create_dir_all(&game).unwrap();
            let paths = Paths::with_app_data(home, root.join("appdata"));
            Sandbox { root, core: Core::with_paths(paths, "test".into()), game, steam }
        }

        pub fn depot(&self) -> PathBuf {
            let d = self.steam.join("steamapps/content/app_311210/depot_311211");
            std::fs::create_dir_all(&d).unwrap();
            d
        }

        pub fn log(&self) -> String {
            self.core.log.read_file()
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}
