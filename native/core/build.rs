fn main() {
    // Release workflows update package.json before building all deliverables.
    // Embed the same stable/beta version without a runtime resource lookup.
    println!("cargo:rerun-if-changed=../../package.json");
    let package: serde_json::Value = serde_json::from_str(include_str!("../../package.json"))
        .expect("package.json must contain valid release metadata");
    let version = package["version"]
        .as_str()
        .expect("package.json version is required");
    println!("cargo:rustc-env=PATCHOPSIII_VERSION={version}");
}
