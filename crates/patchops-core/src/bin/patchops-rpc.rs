//! GUI-free JSON-lines RPC server over stdin/stdout: same protocol as `patchops --rpc-stdio`
//! but builds in seconds (no gpui). Used for development and `tools/parity.py`.

fn main() {
    patchops_core::rpc::serve_stdio(&patchops_core::Core::new());
}
