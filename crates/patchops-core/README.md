# patchops-core

Pure logic of the PatchOpsIII port: no GUI, no async runtime, blocking `std` I/O. Deps: `serde`, `serde_json`
(`preserve_order`), `libc` (unix only), `ureq` (rustls/ring, bundled Mozilla roots, no OpenSSL) and `flate2`
(pure-Rust miniz_oxide) for the network installers. Contract: `bench/OPS.md`; reference behaviour: the legacy Python
(`backend/api.py`, `utils.py`, ...) and `bench/golden/`.

## Map
`context::Core` (paths, log, cached settings, Steam cache) · `ops/` (one file per group, `ops::dispatch`) ·
`state::State` (assembly point; each group owns its sub-struct + builder) · `args` validation in `ops/args.rs` ·
`log` (file + ring of 300 + `subscribe`) · `settings` (`electron-settings.json`, Python-identical JSON) ·
`steam/` (roots, libraries, user id, workshop state, launch-option read, `vdf` parser/writer) · `game`, `backup`,
`config_ini`, `presets`, `hash` (sha256), `pyfmt`/`pyjson` (Python-compatible formatting) · `rpc` (JSON-lines transport).

## Adding a handler (group agents edit only their own `ops/<group>.rs`)
1. Typed fn: `pub fn set_x(core: &Core, ...) -> AppResult<State>`; failures are `Err(AppError::new("legacy text"))`
   (`{"ok":false,"error":..}`); `.with_state(..)` / `.with_extra(..)` for the odd legacy shapes.
2. In the group's `call`: validate with `Args` (`str_req/str_or/str_opt/bool_req/bool_or/bool_opt/int_range/config_value`;
   extra keys ignored; failures are `OpError::Validation`), then `state_reply(set_x(..))` or `reply(..)`/`to_json(..)`.
3. Log through `core.log.success/info/warning/error` (file + ring + subscribers; `log_file_only` for the legacy
   `write_log(.., None)` cases). Settings: `core.settings()` / `core.update_settings(|s| ..)`.
4. State: your `*_state(cx: &StateCx)` builder returns the typed sub-struct (serde camelCase renames).

## Network ops (`update-check`, `dxvk-install`, `t7-install`, `enhanced-install`)
`net` (blocking `ureq` client: proxy env vars, `SSL_CERT_FILE`/`REQUESTS_CA_BUNDLE`, progress callback, streaming
SHA-256), `archive` (zip + tar.gz extraction with zip-slip / symlink / size guards; zip via `enhanced::zip`),
`t7_install`, `enhanced_net`, `elevate` + `cli` (Windows UAC re-launch with the hidden `--install-t7-patch`
command; the shell calls `cli::run_cli_command(&args)` first thing in `main`). Every installer has a typed
`*_with(core, &Net, ..., progress)` variant; tests point `Net::local(base)` at a loopback `TcpListener`
(`net::testserver`), nothing in `cargo test` touches the internet.

## Run
```
cargo test -p patchops-core                      # unit + dispatch tests
cargo build -p patchops-core --bin patchops-rpc  # GUI-free RPC server, builds in seconds
echo '{"id":1,"op":"status"}' | target/debug/patchops-rpc
```

## Parity against the recorded goldens
`tools/parity.py` replays all 161 golden steps headless in a sandbox (needs the bench fixture; run as root is fine,
the process drops to uid 65534) and reports only what you select:
```
python3 tools/parity.py --ops intro-skip,all-intros-skip,d3dcompiler --keep-state qol --ignore-log -v
python3 tools/parity.py --labels '^t7-' --keep-state t7 --ignore-log
python3 tools/parity.py --bin target/release-fast/patchops      # test the real GUI binary (--rpc-stdio)
```
`--keep-state <keys>` compares only those top-level `state` keys (others belong to groups that may not be ported
yet); `--ignore-log` drops the whole-log hash / `maintenance.logPayload` (they cascade from any unported op) but still
compares the log lines each step appends. Default binary: `target/debug/patchops-rpc`. Options: `-q`, `-v`, `--keep`.

## Log retention

`<appdata>/PatchOpsIII.log` is size-bounded (`log::LOG_MAX_BYTES` / `LOG_KEEP_BYTES`): when it exceeds 2 MiB the newest 512 KiB,
cut forward to a whole line, stay in the log and the older content moves to `PatchOpsIII.log.1` (a single generation, replaced
on each trim, same permission bits as the log, never part of the copy-logs payload; `Clear Logs` removes it too). Below the cap
nothing is touched. The cap is checked on append (in-memory size, no extra syscalls per append) and, only in the GUI process, once
at start-up from a background thread (`Logger::trim_if_needed`; `Logger::new`/`Core::new` never touch the file, so nothing
delays the first frame). The hidden elevated CLI child never trims (`Logger::disable_trim`) and the GUI's panic hook uses the
append-only `log::append_line`. A trim takes a `PatchOpsIII.log.trim` lock file (so two processes never trim at once), prepares
the new `.1` and the new log completely, and carries every line appended to the old file while it worked into the new log, so
no line is lost (a line written during the final swap can land a few lines out of order; abandoned or failed attempts can leave
duplicates across `.1` and the log, never gaps). If the trim cannot be done (unwritable directory, Windows rename against an
open handle) the log is left as is and retried only after it grows another MiB (64 KiB after an abandoned attempt); the back-off
is dropped as soon as the real size is seen at or below the cap again. A single line longer than the retained size rotates out
whole. `maintenance.logPayload` is a lazy value (`ops::basics::
LogPayload`): building a `State` only captures a handle, the text is produced from a per-`Logger` cache (validated by one
`stat` of `(len, mtime)` per use, invalidated on append/clear/trim) when something serialises it. The RPC/JSON path (and the
GUI's in-process bench RPC) therefore still returns the full field, byte-identical to before; the GUI's typed refreshes
never force it. Legacy's every-3rd-launch wipe (`manage_log_retention_on_launch`, never called there) was not adopted.
