fn main() {
    println!("cargo:rerun-if-changed=../package.json");
    if let Ok(package) = std::fs::read_to_string("../package.json") {
        if let Some(version) = serde_json::from_str::<serde_json::Value>(&package)
            .ok()
            .and_then(|value| value.get("version")?.as_str().map(str::to_owned))
        {
            println!("cargo:rustc-env=PATCHOPSIII_VERSION={version}");
        }
    }
    tauri_build::build();
}
