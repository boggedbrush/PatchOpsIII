//! Python main is the oracle. See parity/README.md for isolation and normalization.
#![cfg(target_os = "linux")]
use patchops_core::{AppState, Engine};
use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn decode64(text: &str) -> Vec<u8> {
    let mut accumulator = 0u32;
    let mut bits = 0;
    let mut bytes = Vec::new();
    for byte in text.bytes().filter(|byte| *byte != b'=') {
        let value = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
            .iter()
            .position(|candidate| *candidate == byte)
            .expect("valid fixture base64");
        accumulator = (accumulator << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((accumulator >> bits) as u8);
        }
    }
    bytes
}

fn encode64(bytes: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let value = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | chunk.get(2).copied().unwrap_or(0) as u32;
        for offset in (0..4).rev() {
            let index = 3 - offset;
            output.push(if index > chunk.len() {
                '='
            } else {
                alphabet[((value >> (6 * offset)) & 63) as usize] as char
            });
        }
    }
    output
}

fn normalize_text(text: &str, root: &Path) -> String {
    let mut text = text.replace(root.to_str().unwrap(), "$ROOT");
    if let Ok(kernel) = fs::read_to_string("/proc/sys/kernel/osrelease") {
        text = text.replace(kernel.trim(), "$KERNEL");
    }
    Regex::new(r"\b\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\b")
        .unwrap()
        .replace_all(&text, regex::NoExpand("$TIMESTAMP"))
        .into_owned()
}

fn normalize(value: Value, root: &Path) -> Value {
    match value {
        Value::String(text) => Value::String(normalize_text(&text, root)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| normalize(item, root))
                .collect(),
        ),
        Value::Object(items) => Value::Object(
            items
                .into_iter()
                .map(|(key, value)| (normalize_text(&key, root), normalize(value, root)))
                .collect(),
        ),
        value => value,
    }
}

fn recreate(root: &Path, fixture: &Value) {
    fs::create_dir_all(root).unwrap();
    for directory in fixture["directories"].as_array().unwrap() {
        fs::create_dir_all(root.join(directory.as_str().unwrap())).unwrap();
    }
    for file in fixture["files"].as_array().unwrap() {
        let path = root.join(file["path"].as_str().unwrap());
        let bytes = if let Some(text) = file["text"].as_str() {
            text.replace("$ROOT", root.to_str().unwrap()).into_bytes()
        } else {
            decode64(file["base64"].as_str().unwrap())
        };
        fs::write(&path, bytes).unwrap();
        #[cfg(unix)]
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(file["mode"].as_u64().unwrap() as u32),
        )
        .unwrap();
    }
}

fn snapshot(root: &Path) -> Value {
    fn visit(root: &Path, path: &Path, directories: &mut Vec<String>, files: &mut Vec<Value>) {
        let mut children: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        children.sort();
        for path in children {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            if path.is_dir() {
                directories.push(relative);
                visit(root, &path, directories, files);
            } else {
                let raw = fs::read(&path).unwrap();
                #[cfg(unix)]
                let mode = path.metadata().unwrap().permissions().mode() & 0o777;
                #[cfg(not(unix))]
                let mode = 0o644;
                let mut entry = json!({"path": relative, "mode": mode});
                let normalized = match String::from_utf8(raw.clone()) {
                    Ok(text) => {
                        let text = normalize_text(&text, root);
                        entry["text"] = json!(text);
                        text.into_bytes()
                    }
                    Err(_) => {
                        entry["base64"] = json!(encode64(&raw));
                        raw
                    }
                };
                entry["size"] = json!(normalized.len());
                entry["sha256"] = json!(format!("{:x}", Sha256::digest(&normalized)));
                files.push(entry);
            }
        }
    }
    let mut directories = Vec::new();
    let mut files = Vec::new();
    visit(root, root, &mut directories, &mut files);
    directories.sort();
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    json!({"directories": directories, "files": files})
}

fn differences(case: &str, location: &str, expected: &Value, actual: &Value, count: &mut usize) {
    if expected == actual {
        return;
    }
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            let keys: std::collections::BTreeSet<_> =
                expected.keys().chain(actual.keys()).collect();
            for key in keys {
                if expected.contains_key(key) && actual.contains_key(key) {
                    differences(
                        case,
                        &format!("{location}/{key}"),
                        &expected[key],
                        &actual[key],
                        count,
                    );
                } else {
                    *count += 1;
                    println!(
                        "PARITY_DIFF {}",
                        json!({"case": case, "path": format!("{location}/{key}"), "expected": expected.get(key), "actual": actual.get(key), "expectedPresent": expected.contains_key(key), "actualPresent": actual.contains_key(key)})
                    );
                }
            }
        }
        // Compare filesystem entries by path so one extra log/settings file doesn't
        // shift every subsequent entry and hide the real changes.
        (Value::Array(expected), Value::Array(actual)) if location.ends_with("/files") => {
            let as_map = |files: &[Value]| -> Value {
                Value::Object(
                    files
                        .iter()
                        .map(|file| (file["path"].as_str().unwrap().to_owned(), file.clone()))
                        .collect(),
                )
            };
            differences(case, location, &as_map(expected), &as_map(actual), count);
        }
        (Value::Array(expected), Value::Array(actual)) if expected.len() == actual.len() => {
            for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                differences(
                    case,
                    &format!("{location}/{index}"),
                    expected,
                    actual,
                    count,
                );
            }
        }
        _ => {
            *count += 1;
            println!(
                "PARITY_DIFF {}",
                json!({"case": case, "path": location, "expected": expected, "actual": actual})
            );
        }
    }
}

// Reviewed DXVK adoption differences only. Construct exact expected ownership
// bytes from the Python fixture and expected config, never from Rust output.
fn expected_snapshot(name: &str, golden: &Value, snapshot: &Value) -> Value {
    let mut expected = snapshot.clone();
    if name != "dxvk_config_write" {
        return expected;
    }
    let fixture_file = |name: &str| {
        golden["fixture"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["path"] == format!("game/{name}"))
            .unwrap()
    };
    let config = expected["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "game/dxvk.conf")
        .unwrap();
    let manifest = json!({
        "version": 1,
        "game_dir": "$ROOT/game",
        "files": [
            {"name": "dxgi.dll", "installed_sha256": fixture_file("dxgi.dll")["sha256"], "original_sha256": null},
            {"name": "d3d11.dll", "installed_sha256": fixture_file("d3d11.dll")["sha256"], "original_sha256": null},
            {"name": "dxvk.conf", "installed_sha256": config["sha256"], "original_sha256": fixture_file("dxvk.conf")["sha256"]}
        ]
    });
    let text = serde_json::to_string_pretty(&manifest).unwrap();
    let mut backup = fixture_file("dxvk.conf").clone();
    backup["path"] = json!("data/DXVK Managed/originals/dxvk.conf");
    let files = expected["files"].as_array_mut().unwrap();
    files.push(backup);
    files.push(json!({"path": "data/DXVK Managed/manifest.json", "mode": 0o664,
        "sha256": format!("{:x}", Sha256::digest(text.as_bytes())), "size": text.len(), "text": text}));
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    let directories = expected["directories"].as_array_mut().unwrap();
    directories.extend([
        json!("data/DXVK Managed"),
        json!("data/DXVK Managed/originals"),
    ]);
    directories.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    expected
}

fn run_case(name: &str) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut golden: Value = serde_json::from_slice(
        &fs::read(repo.join(format!("native/core/tests/parity/{name}.json"))).unwrap(),
    )
    .unwrap();
    if let Ok(root) = env::var("PATCHOPS_PARITY_CHILD_ROOT") {
        let root = PathBuf::from(root);
        let state = AppState::new(root.join("data"), Some(root.join("resources")), None).unwrap();
        if name == "dxvk_config_write" {
            // Synthetic fixture payloads stand in for one verified GPLAsync
            // release pair. Production has no override and hashes real DLLs.
            let game = root.join("game");
            state.set_dxvk_legacy_hash_provider(std::sync::Arc::new(move |path| {
                if path == game.join("dxgi.dll") {
                    assert_eq!(fs::read(path).unwrap(), b"\x00\xfffixture-dxgi");
                    Ok("177cea0f3d64ac7a2834e24637aecb4ab133e036c50c2568489079cefb8fd7ec".into())
                } else if path == game.join("d3d11.dll") {
                    assert_eq!(fs::read(path).unwrap(), b"fixture-d3d11");
                    Ok("c9e9d1a7844077df38cd0e540be1b435db7c9fed878a60fc32d22c778345fc09".into())
                } else {
                    Err("unexpected legacy DLL probe".into())
                }
            }));
        }
        // Mirror the Python oracle's disabled Steam lifecycle hooks.
        state.set_steam_lifecycle_callback(std::sync::Arc::new(|_| Ok(())));
        let aliases: std::collections::HashMap<PathBuf, String> = golden["fixture"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|file| {
                Some((
                    root.join(file["path"].as_str()?),
                    file["logicalSha256"].as_str()?.to_owned(),
                ))
            })
            .collect();
        fn strip_alias_metadata(value: &mut Value) {
            match value {
                Value::Object(object) => {
                    object.remove("logicalSha256");
                    for value in object.values_mut() {
                        strip_alias_metadata(value);
                    }
                }
                Value::Array(array) => {
                    for value in array {
                        strip_alias_metadata(value);
                    }
                }
                _ => {}
            }
        }
        strip_alias_metadata(&mut golden);
        let engine = Engine::new(state).with_exe_hash_provider(std::sync::Arc::new(move |path| {
            aliases.get(path).cloned().map(Ok).unwrap_or_else(|| {
                fs::read(path)
                    .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
                    .map_err(|e| e.to_string())
            })
        }));
        let mut count = 0;
        let internal = golden["internalOperation"].as_str() == Some("t7_release_asset_discovery");
        if internal {
            let actual = golden["expected"]["responses"][0]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| {
                    let mut item = item.clone();
                    let asset = engine
                        .resolve_t7_release_asset(
                            item["assetKey"].as_str().unwrap(),
                            &item["release"],
                        )
                        .unwrap();
                    item["resolved"] = json!({"url": asset.url, "sha256": [asset.sha256]});
                    item
                })
                .collect::<Vec<_>>();
            differences(
                name,
                "responses/0",
                &golden["expected"]["responses"][0],
                &json!(actual),
                &mut count,
            );
        }
        for (index, operation) in golden["operations"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|_| !internal)
        {
            let body = operation.get("body").map(|body| {
                serde_json::from_str::<Value>(
                    &body.to_string().replace("$ROOT", root.to_str().unwrap()),
                )
                .unwrap()
            });
            let actual = engine
                .dispatch(operation["path"].as_str().unwrap(), body)
                .unwrap_or_else(|error| json!({"dispatchError": error}));
            differences(
                name,
                &format!("responses/{index}"),
                &golden["expected"]["responses"][index],
                &normalize(actual, &root),
                &mut count,
            );
            differences(
                name,
                &format!("checkpoints/{index}"),
                &expected_snapshot(name, &golden, &golden["expected"]["checkpoints"][index]),
                &snapshot(&root),
                &mut count,
            );
        }
        differences(
            name,
            "filesystem",
            &expected_snapshot(name, &golden, &golden["expected"]["filesystem"]),
            &snapshot(&root),
            &mut count,
        );
        assert_eq!(
            count, 0,
            "{name}: {count} parity differences; see PARITY_DIFF records"
        );
        return;
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let scratch = env::temp_dir().join(format!(
        "patchops-rust-parity-{}-{unique}-{name}",
        std::process::id()
    ));
    let root = scratch.join("root");
    recreate(&root, &golden["fixture"]);
    let mut physical_fixture = golden["fixture"].clone();
    for file in physical_fixture["files"].as_array_mut().unwrap() {
        file.as_object_mut().unwrap().remove("logicalSha256");
    }
    assert_eq!(
        snapshot(&root),
        physical_fixture,
        "fixture reconstruction must be lossless"
    );
    let tools = scratch.join("tools");
    fs::create_dir_all(&tools).unwrap();
    for (name, exit) in [("steam", 0), ("pkill", 1)] {
        let path = tools.join(name);
        // Change comm to `true` before exiting: a zombie shell named `steam`
        // would make the real /proc polling incorrectly report Steam running.
        let script = if name == "steam" {
            "#!/bin/sh\nexec /usr/bin/true\n".to_owned()
        } else {
            format!("#!/bin/sh\nexit {exit}\n")
        };
        fs::write(&path, script).unwrap();
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let home = env::var_os("HOME").expect("HOME is required for the existing Steam discovery API");
    let home = PathBuf::from(home);
    let output = Command::new("bwrap")
        .args([
            "--die-with-parent",
            "--ro-bind",
            "/",
            "/",
            "--unshare-pid",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
        ])
        .arg("--tmpfs")
        .arg(&home)
        .arg("--ro-bind")
        .arg(&repo)
        .arg(&repo)
        .arg("--bind")
        .arg(&scratch)
        .arg(&scratch)
        .arg("--dir")
        .arg(home.join(".steam"))
        .arg("--symlink")
        .arg(root.join("steam"))
        .arg(home.join(".steam/steam"))
        .args(["/bin/sh", "-c", "umask 002; exec \"$@\"", "parity"])
        .arg(env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("PATCHOPS_PARITY_CHILD_ROOT", &root)
        .env(
            "PATH",
            format!(
                "{}:{}",
                tools.display(),
                env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .expect("bubblewrap must be installed; no unsafe host fallback is permitted");
    fs::remove_dir_all(scratch).unwrap();
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    assert!(
        output.status.success(),
        "{name}: isolated parity test failed ({})",
        output.status
    );
}

macro_rules! case {
    ($name:ident) => {
        #[test]
        #[cfg_attr(
            not(target_os = "linux"),
            ignore = "needs a platform filesystem/process isolation adapter"
        )]
        fn $name() {
            run_case(stringify!($name));
        }
    };
}
case!(status_no_game);
case!(status_unverified_base);
case!(status_steam_secondary_library);
case!(game_directory_validation_invalid);
case!(game_directory_validation_valid);
case!(config_graphics_advanced_write);
case!(t7_config_write);
case!(dxvk_config_write);
case!(launch_options_apply);
case!(qol_toggles);
case!(status_t7_installed);
case!(status_dxvk_installed);
case!(status_enhanced_installed);
case!(status_empty_config);
case!(status_config_variants);
case!(config_append_and_crlf);
case!(config_missing_file);
case!(t7_config_missing);
case!(t7_config_clear_and_append);
case!(t7_config_invalid_name);
case!(launch_options_unsupported);
case!(qol_restore_legacy);
case!(qol_toggle_roundtrip);
case!(exe_untrusted_backup_detection);
case!(exe_preserved_enhanced_detection);
case!(status_alternate_executable);
case!(config_readonly_roundtrip);
case!(vram_target_roundtrip);

case!(status_september_2026_current);
case!(status_compatible_build);
case!(t7_release_asset_discovery);
