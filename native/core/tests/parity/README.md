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
other platforms are ignored pending equivalent filesystem/process isolation.

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
   `logicalSha256` alias used **only by the Python hash seam**. Their physical
   sizes/hashes are also recorded. Production code checks build identity by
   SHA-256, not a fixed EXE byte size; fake bytes cannot have the real digest.

## Missing Rust adapters (three explicit ignores)

| Case | Adapter needed |
| --- | --- |
| `status_september_2026_current` | Injectable file-hash provider, covering the September 2026 hash and BuildID 24784313. |
| `status_compatible_build` | Same hash seam, covering the compatible digest and BuildID 10650222. |
| `t7_release_asset_discovery` | Expose a pure asset selector or injectable HTTP transport; the existing selector is private to installation/download. |

These cases contain real Python expected output. Running them explicitly with
`--ignored` fails with the missing adapter reason; they cannot silently pass.

## Results

See [RESULTS.md](RESULTS.md) for concrete regressions and
[results.jsonl](results.jsonl) for **every** leaf mismatch (including intermediate
checkpoints and derivative size/hash differences). Dispatch errors are captured
as `{"dispatchError": ...}` diagnostic values: this wrapper is not a production
API response and is not accepted as equivalent to Python's `ok/error/state`
JSON. The strict suite deliberately remains red while these differences exist.

Status-envelope differences do not prevent the harness from checking the
remaining response fields or filesystem changes.
Byte-only formatting differences are reported alongside functional differences
and should be assessed separately. Do not replace goldens with Rust output to
make the suite green.
