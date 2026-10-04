# Python → Rust behavioral parity

The case JSON files are goldens generated from the Python backend on `main`, not
handwritten Rust expectations. `sourceHashes` records the five Python modules
used. At generation, those modules were identical to local `main`.

Run from the repository root:

```sh
python3 scripts/gen_parity_fixtures.py
python3 scripts/gen_parity_fixtures.py --check
cd native
~/.cargo/bin/cargo test -p patchops-core --test parity -- --nocapture
```

The Python runner prefers FastAPI TestClient when FastAPI, Pydantic, and HTTPX
are available. This environment lacks FastAPI; the existing temporary venv
also has neither pip nor ensurepip. These goldens therefore use direct async
endpoint calls with minimal import/model shims, plus installed `requests` and
`vdf`. They exercise endpoint logic and filesystem effects, **not FastAPI
request validation or lifecycle hooks**. Installing the real dependencies may
change `runner` metadata; regenerate before using `--check` in that environment.

## Coverage

31 cases cover:

- Status without a game, base game, secondary Steam library discovered through
  `libraryfolders.vdf`, both executable names, empty/malformed/decimal graphics
  configuration, installed T7/DXVK/Enhanced, current September 2026 and
  compatible executable profiles.
- Valid and invalid game-directory selection; graphics/advanced writes; config
  append and CRLF input; missing config; read-only/write/unlock; VRAM limit and
  unlimited round trips.
- T7 gamertag/color/password/friends-only edits, missing keys, trimmed values,
  empty/overlong names, missing configuration, preservation of unknown keys.
- DXVK settings writes with existing DLL/config files.
- Supported Steam launch-option application and preservation of existing Wine,
  command marker, custom flags, and `fs_game`; unsupported options.
- Intro, all-intro, and compiler toggles, repeated enable, restoration, and
  legacy `.bak` restoration.
- EXE integrity/state detection, untrusted build backups, and a preserved
  Enhanced hash with real fixture bytes.
- Mock release metadata for version-suffixed legacy T7 archives and renamed
  universal current archives (the #46/#47 behavior).

Each golden records initial directories/files, operations, endpoint responses,
filesystem snapshots **after every operation**, and final filesystem state.
Files include relative path, mode, normalized byte size, SHA-256, and text or
base64 content. Fixture recreation is checked before invoking Rust.

## Isolation

Generation uses temporary roots, fixture settings/presets/Steam paths, disabled
Steam process operations, synchronous log capture, and a fail-closed Requests
network guard. Release discovery alone replaces `requests.get` with a fake
response. No game payload is downloaded or redistributed.

The Rust tests target the public `AppState::new` and `Engine::dispatch` APIs.
On Linux they re-exec each case through **bubblewrap** with a read-only host
filesystem, a temporary home mount, a fixture Steam symlink, writable fixture
root, isolated PID namespace, and fake `steam`/`pkill` commands. The home
environment variable is read, never changed; real Steam files/processes cannot
be affected. No network-performing Rust dispatch is exercised. Missing or
unsupported bubblewrap fails explicitly; there is no host fallback. Tests on
other platforms compile an empty parity test target and skip these Linux-only
cases cleanly, pending equivalent filesystem/process isolation. Linux CI needs
the apt package `bubblewrap`; the native Ubuntu workflow already includes it.

## Normalization rules

1. Replace each temporary root in responses, object keys, and UTF-8 file
   contents with `$ROOT`. Paths in snapshots are relative and use `/`.
2. Replace generated `YYYY-MM-DD HH:MM:SS` log timestamps with `$TIMESTAMP`.
   Seeded ISO Enhanced timestamps are fixed inputs and retained verbatim.
3. Replace the host kernel release in Python log headers with `$KERNEL`.
   Version strings, platform naming, log header structure, messages, and
   category/order are retained and compared.
4. Sort filesystem entries/directories and JSON object keys. Preserve response
   array order, config comments, line endings, permission bits, VDF whitespace,
   JSON file formatting, missing-vs-null fields, and every extra/deleted file.
   JSON numbers are compared exactly, including integer versus float representation.
5. Text file size/hash describe normalized UTF-8 bytes, not the physical
   temporary-path-expanded file. Binary files use unmodified bytes/base64.
   File mtimes/ownership and temporary directory names are not recorded. Both
   runners use fixed umask `002`; fixture files explicitly start at `0644`.
   This catches atomic replacements that change existing permission bits.
6. Known current/compatible EXEs contain nonempty marker bytes and a
   `logicalSha256` alias used by the Python and Rust status hash seams. Their physical
   sizes/hashes are also recorded. Production code checks build identity by
   SHA-256, not a fixed EXE byte size; fake bytes cannot have the real digest.

## Test seams and reviewed migration differences

All 31 cases run on Linux; there are no ignored adapter cases. Historical
`rustIgnore` fixture metadata records the original gaps and does not skip tests.
`Engine::with_exe_hash_provider` injects status hashes, including preserved EXE
probes. Mutation checks always hash physical bytes. `logicalSha256` is adapter
metadata excluded from snapshot comparison; physical sizes/hashes/content still
compare exactly. `select_t7_release_asset` is a public pure JSON selector using
the installer selection logic, and the Engine wrapper retains discovery logs.
Steam lifecycle callbacks mirror the Python runner's no-op process hooks;
production still closes/waits/reopens Steam. No process lifecycle is verified
by these goldens.

DXVK configure first adopts an unbound legacy install only when both root DLLs
are regular, non-symlink files whose SHA-256 pair matches the same known official
GPLAsync release, and root `dxvk.conf` is regular and non-symlink. Unknown,
incomplete, mixed, or unsafe installs remain refused. The manifest records the
canonical game directory and physical installed hashes. The original config is
copied and verified before configuring; uninstall restores it. Existing managed
state must validate before reuse. Filenames/config shape alone never prove
ownership. The fake DLL fixture uses a narrowly scoped legacy recognition hash
provider for the v3.0-1 pair; manifest/backup hashes always use real fixture bytes.

The **only filesystem expected-diff allowlist**, in `expected_snapshot` in
`../parity.rs`, applies to `dxvk_config_write` checkpoints/final state:

- Add `data/DXVK Managed` and `data/DXVK Managed/originals` directories.
- Add `data/DXVK Managed/manifest.json`: version 1, `$ROOT/game` binding, exact
  physical fixture DLL hashes, Python's expected updated config hash, and the
  original fixture config hash. Expected JSON bytes/mode/size/hash are computed
  independently from the fixture and Python golden, never from Rust output.
- Add `data/DXVK Managed/originals/dxvk.conf`: exact original fixture bytes and
  mode. No DLL backup, other extra file/directory, response/log difference, or
  existing-file rewrite is allowed by this policy.

These are intentional ownership records required by the reviewed safety policy.
No Python golden is changed or regenerated to accept Rust behavior.

Enhanced legacy status metadata is bound **in memory per AppState** on first
matching status read: legacy state must be installed with nonempty recorded
files, all four Enhanced markers and all recorded files must be regular files
inside the current game directory, paths must be safe and belong to the Enhanced
or root dump whitelist, and recorded hashes (if present) must match. Partial
modern ownership metadata is refused. The first canonical binding cannot follow
a second game directory in the same AppState. Counts, backup status, and timestamps
are preserved. The JSON file is left byte-for-byte intact, keeping Python status
reads free of filesystem writes. This display binding does not authorize
install/uninstall; the existing verified archive/ownership adoption still applies.
A durable binding across app restarts would require a separately reviewed
Enhanced filesystem allowlist, which this task did not authorize.

## Results

See [RESULTS.md](RESULTS.md) for concrete regressions and
[results.jsonl](results.jsonl) for **every** leaf mismatch (including intermediate
checkpoints and derivative size/hash differences). Dispatch errors are captured
as `{"dispatchError": ...}` diagnostic values: this wrapper is not a production
API response and is not accepted as equivalent to Python's `ok/error/state`
JSON. The latest isolated run passes all 31 cases with the exact DXVK ownership
allowlist above.

Status-envelope differences do not prevent the harness from checking the
remaining response fields or filesystem changes.
Byte-only formatting differences are reported alongside functional differences
and should be assessed separately. Do not replace goldens with Rust output to
make the suite green.
