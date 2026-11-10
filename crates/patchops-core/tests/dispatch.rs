//! Black-box tests of `ops::dispatch` in a throw-away HOME (no Steam, no game).

use patchops_core::paths::Paths;
use patchops_core::{ops, Core, OpError};
use serde_json::{json, Value};

fn core() -> (Core, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("patchops-core-test-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let paths = Paths::with_app_data(home, root.join("appdata"));
    (Core::with_paths(paths, "test".into()), root)
}

#[test]
fn unknown_and_validation_errors() {
    let (c, root) = core();
    assert!(matches!(ops::dispatch(&c, "nope", &Value::Null), Err(OpError::UnknownOp(_))));
    let e = ops::dispatch(&c, "game-directory", &json!({})).unwrap_err();
    assert!(e.to_string().starts_with("validation:"), "{e}");
    let e = ops::dispatch(&c, "game-directory", &json!({"path": ""})).unwrap_err();
    assert!(e.to_string().starts_with("validation:"), "{e}");
    let e = ops::dispatch(&c, "release-channel", &json!({"channel": 3})).unwrap_err();
    assert!(e.to_string().starts_with("validation:"), "{e}");
    let e = ops::dispatch(&c, "intro-skip", &json!({"enabled": "maybe"}));
    // intro-skip is not ported yet, but its args are validated by the group once ported
    assert!(e.is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn health_status_and_release_channel() {
    let (c, root) = core();
    assert_eq!(ops::dispatch(&c, "health", &json!({})).unwrap(), json!({"ok": true, "version": "test"}));
    let st = ops::dispatch(&c, "status", &Value::Null).unwrap();
    assert_eq!(st["gameDetected"], json!(false));
    assert_eq!(st["releaseChannel"], json!("stable"));
    assert_eq!(st["launchProfiles"].as_array().unwrap().len(), 5);
    let r = ops::dispatch(&c, "release-channel", &json!({"channel": " BETA ", "ignored": 1})).unwrap();
    assert_eq!(r["ok"], json!(true));
    assert_eq!(r["state"]["releaseChannel"], json!("beta"));
    let settings = std::fs::read_to_string(&c.paths.settings).unwrap();
    assert_eq!(settings, "{\n  \"release_channel\": \"beta\"\n}");
    let bad = ops::dispatch(&c, "release-channel", &json!({"channel": "nightly"})).unwrap();
    assert_eq!(bad, json!({"ok": false, "error": "Release channel must be beta or stable."}));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn logs_clear_and_mod_files() {
    let (c, root) = core();
    c.log.info("hello");
    let p = ops::dispatch(&c, "logs/payload", &Value::Null).unwrap();
    assert!(p["payload"].as_str().unwrap().contains("Info: hello"));
    let r = ops::dispatch(&c, "logs/clear", &Value::Null).unwrap();
    assert_eq!(r["ok"], json!(true));
    let logs = r["state"]["logs"].as_array().unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["message"], json!("Logs cleared."));
    std::fs::create_dir_all(c.paths.mod_files.join("sub")).unwrap();
    std::fs::write(c.paths.mod_files.join("a.zip"), b"x").unwrap();
    assert_eq!(ops::dispatch(&c, "mod-files/clear", &Value::Null).unwrap()["ok"], json!(true));
    assert_eq!(std::fs::read_dir(&c.paths.mod_files).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(root);
}
