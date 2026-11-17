# patchops-core

Pure logic of the PatchOpsIII port: no GUI, no async runtime, blocking `std` I/O. Deps: `serde`, `serde_json`
(`preserve_order`), `libc` (unix only), `ureq` (rustls/ring, bundled Mozilla roots, no OpenSSL) and `flate2`
(pure-Rust miniz_oxide) for the network installers. Contract: `bench/OPS.md`; reference behaviour: the legacy Python
(`backend/api.py`, `utils.py`, ...) and `bench/golden/`.

Rule: no `unwrap`/`expect`/`panic!`/`unreachable!`/`todo!` in non-test code, and every `unsafe` block has a `// SAFETY:` comment; CI's clippy step (`-D warnings`) enforces both via `[workspace.lints.clippy]`.

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

## Differential fuzzing against the legacy backend (`tools/difffuzz.py`)

`tools/difffuzz.py` runs random op sequences against the legacy Python backend (baseline `2503485`, see the tool's docstring
for the setup) and this port on byte-identical sandboxes and compares every response and the whole filesystem after every op.
Minimised repros of what it found live in `tools/difffuzz_corpus/*.json` (`python3 tools/difffuzz.py --replay <file>`); the
legacy backend is not available in CI, so the essence of every fixed case is also a unit test next to the code.

Where the port follows Python on purpose, and how (all of it is pinned by unit tests):

* **Writes keep the legacy `open(path, "w")` semantics, crash-safe where possible** (`fsutil::atomic_write` / `atomic_copy`): the
  destination is resolved through symlinks first (a dangling link resolves to its target, which is created, like `open`); a file
  with other hard links, or a directory that refuses the temp file / rename (EACCES/EPERM/EROFS), is overwritten in place
  (truncate + write + fsync); otherwise temp file next to the real file + fsync + rename with the existing permission bits; a
  read-only file fails with `[Errno 13] Permission denied` and is left untouched. `copy2` fills the copy before sealing its
  permissions and `copy2_named` says which side failed. The log trim follows a symlinked log and trims the real file.
  Deliberately left: the exe swap replaces a read-only `BlackOps3.exe` (legacy `copy2` raised EACCES); a log trim does not
  special-case hard links or a read-only log directory (it just fails and backs off).
* **Numbers are Python's.** serde_json's `arbitrary_precision` keeps the literal of every JSON number (no new crate: the
  feature only changes `Number`'s representation), so `config` values are `str(int)` for integer literals of any size (`-0` is
  `0`) and `repr(float)` otherwise (`1e400` is `inf`), JSON written by the port uses the same, `pyfmt::{py_int, py_float, PyInt}`
  implement `int()` / `float()` (whitespace, underscores, Unicode digits, `inf`/`nan`) for `config.ini` and `dxvk.conf` values,
  and state integers that Python would show unbounded are `PyInt` (serialised as JSON integers). A request line that is not
  valid JSON for serde_json but looks like an object still gets its `id` echoed.
* **pydantic v2 lax validation** (`ops/args.rs`): bool accepts true/false, `0`/`1`/`0.0`/`1.0` and the strings
  `0 1 t f true false y n yes no on off` in any case without surrounding whitespace; int accepts integers, booleans, floats with a
  zero fraction and strings (`" 8_0 "`, `"80.0"`, `"+5"`); the table test `pydantic_lax_table` holds 120 inputs whose expectations
  were generated by `tools/gen_pydantic_table.py` against pydantic 2.14.
* **Python string semantics** (`pyfmt`): `py_strip` / `py_isspace` (includes U+001C..U+001F), `py_splitlines`, `universal_newlines`
  (text-mode reads: `t7patch.conf`, the log payload, `dxvk.conf`), `repr_str` escapes non-printable characters exactly like
  `repr()` (exact for Unicode 15.1, CPython 3.13: table generated by `tools/gen_pyunicode.py`). A log that is not valid UTF-8 gives
  the `(no log entries found)` payload, as the legacy `read_text` raised. `Path.glob("*.mkv")` matches dotfiles and a `video` that is
  a file behaves as an empty directory. Error texts match Python where it is one formatting of the same failure (`rmtree` naming,
  `[Errno N] <strerror>: '<path>'`, python-vdf's `vdf.parse: <msg> (<file>, line N)`).

Intentional differences the fuzzer reports as *allowlisted* (never as passes; each is an entry in `ALLOWLIST` of `tools/difffuzz.py`
with a narrow matcher):

| id | legacy | port | why |
|---|---|---|---|
| `legacy-500-numeric-config` | `status`/`config` raise HTTP 500 for `inf`/`nan`/`1e999` in numeric config keys (`int(float('inf'))`, `int(nan*100)`) | tolerant defaults | a crash is not a contract |
| `legacy-500-t7-conf` | `t7patch.conf` that is a directory or invalid UTF-8 makes the state builder raise | reads what it can | same |
| `legacy-500-launchoptions-section` | `LaunchOptions` stored as a VDF section makes the state builder raise | treated as no launch options | same |
| `legacy-500-browse-root` | `browse` of `/` raises | lists the root (the directory browser's root button) | same |
| `legacy-500-game-directory` | `game-directory` with a path beyond PATH_MAX / a >255-byte component, or below a directory without search permission (`Path.is_dir` raises), raises | `ok:false` with the reason | same |
| `legacy-500-settings-save` | an unwritable app data dir, or a settings path that is a directory, makes the settings writers (`game-directory`, `enhanced-validate`, ...) raise | `ok:false` with the errno text | same |
| `combined-explained-differences` | - | - | one response whose differences are each explained by a different allowlisted feature of the case |
| `vdf-lenient-localconfig` | python-vdf only looks at the first character of a line: `{ garbage }` parses to `{}`, `}}` closes one level | refuses to parse such a `localconfig.vdf` | rewriting a Steam file that could not be parsed faithfully is the riskier behaviour; ops that must edit it log `Error updating launch options: vdf: ... the file was left unchanged because PatchOpsIII cannot parse it safely` and answer `ok:false`, the backup is restored, read-only state building stays tolerant |
| `rmtree-symlinked-dir` | `shutil.rmtree` refuses a symlink to a directory (`mod-files/clear`, the cache dirs of `t7-uninstall`) and the op fails | unlinks the link, never touches (or follows into) the target | pinned by `mod_files_clear_unlinks_symlinks_and_never_touches_their_targets` and `uninstall_unlinks_a_symlinked_cache_dir_and_keeps_its_target` |
| `settings-hand-edited-types` | echoes a non-string `enhanced_dump_source` from a hand-edited settings file | shows `""` | only reachable by editing the file by hand |
| `null-args-body-op` | `args: null` for an all-optional body model: 422 | treated as `{}` | `bench/OPS.md` section 1: absent args = `{}` |
| `vdf-string-and-section` | python-vdf lets a key be a string and a section | refuses the file | documented in `steam/vdf.rs` |

Steam VDF files: `steam::vdf::python_syntax_error` is a port of python-vdf 3.4's line-oriented `parse` (verdict, message and
line number only; differential-tested against the real library on 46000 generated texts, `python_syntax_error_matches_python_vdf`,
run with `PYVDF_CASES=<json>` and `--ignored`). `vdf::load_file` (every op that rewrites `localconfig.vdf` / `config.vdf`) and
`vdf::parse_checked` (state building) refuse what python-vdf refuses, with its wording (`vdf.parse: <msg> (<file>, line N)`). The
port's own tokenizer still decides the structure, and it is stricter than python-vdf in one direction: a file python-vdf accepts
leniently (`{ garbage }` -> `{}`, a line `}}`) is refused too (`vdf-lenient-localconfig` above); such a file is never rewritten, the
op answers `ok:false` and logs `vdf: <reason> (localconfig.vdf); the file was left unchanged because PatchOpsIII cannot parse it safely`.
