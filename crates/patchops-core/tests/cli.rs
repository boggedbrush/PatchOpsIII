use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

use serde_json::{json, Value};
use tempfile::tempdir;

fn core_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_patchops-core"))
}

fn run_core(args: &[&str], input: Value) -> Output {
    let mut child = Command::new(core_bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn patchops-core");

    child
        .stdin
        .as_mut()
        .expect("patchops-core stdin")
        .write_all(input.to_string().as_bytes())
        .expect("write JSON stdin");

    child.wait_with_output().expect("wait for patchops-core")
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("patchops-core stdout JSON")
}

#[test]
fn hash_command_reads_json_stdin_and_writes_json_stdout() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    std::fs::write(&file, b"patchops").unwrap();

    let output = run_core(&["hash"], json!({ "path": file.to_string_lossy() }));

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let json = stdout_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(
        json["sha256"],
        "3e0cec93c1876296af254daff5dba161649c003683320e5fefbaf1ad21e67f98"
    );
}

#[test]
fn status_command_reads_json_stdin_and_writes_structured_status() {
    let dir = tempdir().unwrap();
    let players = dir.path().join("players");
    std::fs::create_dir_all(&players).unwrap();
    std::fs::write(dir.path().join("BlackOps3.exe"), b"exe").unwrap();
    std::fs::write(players.join("config.ini"), b"MaxFPS = \"165\"").unwrap();

    let output = run_core(
        &["status"],
        json!({ "gameDir": dir.path().to_string_lossy() }),
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let json = stdout_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["gameDetected"], true);
    assert_eq!(json["configExists"], true);
    assert_eq!(json["executableName"], "BlackOps3.exe");
    assert_eq!(json["t7ConfigExists"], false);
    assert_eq!(
        json["executableHash"],
        "9095bdb859308b62acf04036ffd4adfe366d7f737d276eb6c46ae434f3816c9b"
    );
}

#[test]
fn envelope_mode_supports_command_and_payload_on_stdin() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.bin");
    std::fs::write(&file, b"patchops").unwrap();

    let output = run_core(
        &[],
        json!({
            "command": "hash",
            "payload": { "path": file.to_string_lossy() }
        }),
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(stdout_json(&output)["ok"], true);
}

#[test]
fn unknown_command_fails_with_stderr_and_no_stdout() {
    let output = run_core(&["unknown"], json!({}));

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command: unknown"));
}
