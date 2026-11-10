//! Windows: embed the application icon and VERSIONINFO (Explorer "Details" tab, UAC prompt, MSI
//! ARP entry). A no-op on every other host, so Linux builds pay nothing for it.
//!
//! The resource script deliberately contains no RT_MANIFEST: gpui's `windows-manifest` feature
//! already embeds the DPI-awareness / common-controls manifest as resource 1, and a second one
//! would be a duplicate-resource link error.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../packaging/assets/PatchOpsIII.ico");
    #[cfg(windows)]
    windows::embed();
}

#[cfg(windows)]
mod windows {
    use std::path::PathBuf;

    pub fn embed() {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            return;
        }
        let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let icon = manifest_dir.join("../../packaging/assets/PatchOpsIII.ico");
        let icon = icon.canonicalize().expect("packaging/assets/PatchOpsIII.ico is missing");
        // rc.exe wants forward slashes (or doubled backslashes) in string literals; strip the `\\?\` prefix.
        let icon = icon.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");

        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        let numeric: Vec<u16> = version
            .split(['-', '+'])
            .next()
            .unwrap_or("0")
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .chain(std::iter::repeat(0))
            .take(4)
            .collect();
        let (a, b, c, d) = (numeric[0], numeric[1], numeric[2], numeric[3]);

        let rc = format!(
            r#"#pragma code_page(65001)
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},{d}
PRODUCTVERSION {a},{b},{c},{d}
FILEOS 0x40004
FILETYPE 0x1
{{
    BLOCK "StringFileInfo"
    {{
        BLOCK "040904B0"
        {{
            VALUE "CompanyName", "boggedbrush"
            VALUE "FileDescription", "PatchOpsIII"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "PatchOpsIII"
            VALUE "LegalCopyright", "MIT License"
            VALUE "OriginalFilename", "PatchOpsIII.exe"
            VALUE "ProductName", "PatchOpsIII"
            VALUE "ProductVersion", "{version}"
        }}
    }}
    BLOCK "VarFileInfo"
    {{
        VALUE "Translation", 0x0409, 0x04B0
    }}
}}
"#
        );
        let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("patchopsiii.rc");
        std::fs::write(&out, rc).unwrap();
        embed_resource::compile(&out, embed_resource::NONE).manifest_optional().unwrap();
    }
}
