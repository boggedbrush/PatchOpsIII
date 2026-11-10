//! GUI-free JSON-lines RPC server over stdin/stdout: same protocol as `patchops --rpc-stdio`
//! but builds in seconds (no gpui). Used for development and `tools/parity.py`.

fn main() {
    // Hidden elevated-child commands (Windows T7 Patch install) run instead of the server.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = patchops_core::cli::run_cli_command(&args) {
        std::process::exit(code);
    }
    patchops_core::rpc::serve_stdio(&patchops_core::Core::new());
}
