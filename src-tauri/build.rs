fn main() {
    println!("cargo:rerun-if-changed=../package.json");
    if let Ok(content) = std::fs::read_to_string("../package.json") {
        if let Some(version) = serde_json::from_str::<serde_json::Value>(&content)
            .ok()
            .and_then(|package| {
                package
                    .get("version")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
        {
            println!("cargo:rustc-env=PATCHOPSIII_PACKAGE_VERSION={version}");
        }
    }

    tauri_build::build();
}
