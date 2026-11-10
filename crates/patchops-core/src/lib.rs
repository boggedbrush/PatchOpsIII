//! Pure application logic for PatchOpsIII. No GUI dependencies.

/// Crate version, reported by the `health` RPC op.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
