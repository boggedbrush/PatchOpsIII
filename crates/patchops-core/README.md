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
append-only `log::append_line`. An append under the cap is exactly `open(O_APPEND)`, `write`, `close` (no stat of the log).
A trim takes an OS advisory lock on a persistent `PatchOpsIII.log.lock` (`flock` / `LockFileEx`, non-blocking: if another
process holds it this trim is skipped and retried after 64 KiB of growth; the kernel drops the lock if the holder crashes; the
file itself is created only when a trim is attempted and may stay). While holding it a trim first deletes the `.PatchOpsIII.log.tmp-*` /
`.PatchOpsIII.log.1.tmp-*` leftovers of a crashed trim, then reads only the retained tail and streams the old head into the `.1`
temp through a bounded buffer (peak memory stays around the tail, ~1 MiB, even for a 500 MiB log), prepares the new log
completely, and carries every whole line appended to the old file while it worked into the new log (before the two renames and
for a ~16 ms grace period after them), so no line is lost. A crash between the renames leaves duplicates across `.1` and the
log, never gaps. `Clear Logs` takes the same lock (waiting up to 10 s for a trim in flight), so a concurrent trim in another
process can never bring cleared content back. Lock order everywhere: the in-process logger mutex, then the OS lock.
Accepted limits: a line written by ANOTHER process that is stalled for more than ~16 ms between open and write at the instant
of a trim can be lost; each logger counts only its own appends toward the cap, so several processes may overshoot it a little;
the append that crosses the cap performs the trim inline (tens of ms, once per ~1.5 MiB of logging). If the trim cannot be done
(unwritable directory, disk full, Windows rename against an open handle) the log is left as is and retried only after it grows
by half its size, at least 1 MiB (64 KiB after an abandoned attempt or a busy lock); the back-off is dropped as soon as the
real size is seen at or below the cap again. A single line longer than the retained size rotates out
whole. `maintenance.logPayload` is a lazy value (`ops::basics::
LogPayload`): building a `State` only captures a handle, the text is produced from a per-`Logger` cache (validated by one
`stat` of `(len, mtime)` per use, invalidated on append/clear/trim) when something serialises it. The RPC/JSON path (and the
GUI's in-process bench RPC) therefore still returns the full field, byte-identical to before; the GUI's typed refreshes
never force it. Legacy's every-3rd-launch wipe (`manage_log_retention_on_launch`, never called there) was not adopted.
