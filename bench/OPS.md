# PatchOps benchmark: op catalogue and driver contract

This is the contract that the Rust + GPUI port (`target: gpui`) must implement so the benchmark harness can drive it,
and so the behavioural-parity suite (`bench/golden/`) can be replayed against it. It was derived by reading the legacy
Python backend (`backend/api.py`, `utils.py`, `t7_patch.py`, `dxvk_manager.py`, `bo3_enhanced.py`) at the baseline commit
recorded in `bench/results/legacy.json` and verified by running it.

The golden files in `bench/golden/` are the ground truth where this text and the legacy behaviour disagree.

## 1. Transport

Binary: `target/release/patchops` (override with `--gpui-bin`).

| mode | how it is started | purpose |
|---|---|---|
| headless RPC | `patchops --rpc-stdio` | no window, no GPU; serves the protocol on stdin/stdout |
| GUI + RPC | `PATCHOPS_BENCH_RPC=1 patchops` (env `DISPLAY` set, no args) | the normal GUI process, which **also** serves the protocol on stdin/stdout, so the workload runs inside the live GUI process whose memory is measured |

Protocol: UTF-8 JSON lines (one JSON object per line, `\n` terminated) on stdin (requests) and stdout (responses).
Nothing but protocol lines may be written to stdout (log to stderr). Requests are processed in order; one request in flight at a time
(the harness is synchronous), but a server may answer out of order as long as `id` matches.

```
request : {"id": 7, "op": "intro-skip", "args": {"enabled": true}}
success : {"id": 7, "ok": true,  "result": { ...the op's JSON body, exactly as the legacy HTTP API returned it... }}
failure : {"id": 7, "ok": false, "error": "validation: ..."}
```

* **Two levels of `ok`.** Envelope `ok:true` means the op executed; `result` is then the legacy JSON body, which for most ops is itself
  `{"ok": true|false, "error"?: str, "state"?: {...}}`. An application-level failure (`{"ok": false, "error": "config.ini not found at ..."}`)
  is still an envelope `ok:true`.
* **Envelope `ok:false`** is only for: invalid arguments (`error` must start with `validation:` - the legacy API answered HTTP 422),
  unknown op (`unknown op:`), and unexpected internal failure (`internal:`). The golden files only record the class (the text before the first `:`).
* `args` is the legacy JSON body (POST) or query parameters (GET). Absent `args` = `{}`.
* Unknown extra args must be ignored (pydantic default). Types are pydantic-v2 "lax": the harness only sends exact JSON types, but note
  `enabled`/`limited` booleans must reject non-boolean junk such as `"maybe"` with a `validation:` error.
* **Readiness.** The harness sends a single `status` request right after spawn and treats the first answer as "app ready"
  (headless `health_ms` and `ready_ms` are the same instant for gpui). In GUI mode the window must be mapped independently; the harness
  polls X11 for the first viewable top-level window >= 200x150 (title should be `PatchOpsIII`).
* The legacy `GET /ws` log stream is **not** part of the contract (not benchmarked); recent log lines are still returned in `status.logs`
  (ignored by the golden comparison because the legacy implementation fills it asynchronously).
* On EOF of stdin the process should exit. The harness kills the whole process tree after every run and fails the run if anything survives.

## 2. Sandbox environment (identical for both targets)

Every run gets a fresh sandbox `<SB>` (`bench/.work/sandboxes/<kind>-<n>/`, recorded in goldens as the literal `<SB>`).
The target process is started with exactly this environment (plus `DISPLAY` for GUI runs; plus `PATCHOPSIII_BACKEND_PORT` for legacy only):

| variable | value |
|---|---|
| `HOME` | `<SB>/home` |
| `XDG_DATA_HOME` | `<SB>/home/.local/share` (app data dir = `$XDG_DATA_HOME/PatchOpsIII`) |
| `XDG_CONFIG_HOME` / `XDG_CACHE_HOME` / `XDG_STATE_HOME` | `<SB>/home/.config` / `.cache` / `.local/state` |
| `XDG_RUNTIME_DIR` | `<SB>/run` (0700) |
| `TMPDIR` | `<SB>/tmp` |
| `PATH` | `<SB>/fakebin:` + the harness `PATH` (see fake binaries) |
| `LANG`/`LC_ALL`/`TZ` | `C.UTF-8` / `C.UTF-8` / `UTC` |
| `PATCHOPSIII_VERSION_OVERRIDE` | `bench` (the app must report this as `appVersion`/`health.version`) |
| `PATCHOPS_BENCH_FAKESTEAM_LOG` | `<SB>/fakebin-log/steam.calls` |
| `PATCHOPS_BENCH_TAG`, `PATCHOPS_BENCH_SANDBOX` | harness bookkeeping (process sweep) |
| `NO_PROXY` | `*` and all `*_PROXY` removed (offline workload); the `--network` group keeps the proxy/CA variables |
| `PYTHONDONTWRITEBYTECODE` | `1` |

The process runs as an **unprivileged uid (65534)** when the harness is started as root. This matters: `config-readonly` is implemented with
`chmod` and the legacy `configReadonly` flag is `not os.access(path, W_OK)`, which is always false for root. Goldens were recorded as non-root.

The port must resolve all locations from `HOME`/`XDG_*` exactly like the legacy code (Linux only is benchmarked):

| thing | path |
|---|---|
| app data dir | `$XDG_DATA_HOME/PatchOpsIII` |
| settings | `<appdata>/electron-settings.json` (2-space-indented JSON object; keys used: `game_dir`, `release_channel`, `enhanced_dump_source`, `enhanced_exe_hashes`) |
| log file | `<appdata>/PatchOpsIII.log` (lines `YYYY-MM-DD HH:MM:SS - <Category>: <message>`; `Info`, `Success`, `Warning`, `Error`) |
| mod files dir | `<appdata>/BO3 Mod Files` |
| Steam backups | `<appdata>/backups/` (`localconfig_backup.vdf`, ...) |
| presets | `presets.json` next to the app resources (the repo-root file, unchanged) |
| Steam userdata | `$HOME/.steam/steam/userdata/<first all-digit dir>/config/localconfig.vdf` |
| Steam roots/libraries | `$HOME/.steam/steam`, `$HOME/.local/share/Steam` (if present) + every `path` in `<root>/steamapps/libraryfolders.vdf` |
| game dir search | settings `game_dir` (if it still has an exe) -> `<library>/steamapps/common/Call of Duty Black Ops III` for each library -> `~/.steam/steam/...`, `~/.local/share/Steam/...` |
| game exe names | `BlackOps3.exe`, `BlackOpsIII.exe` (first existing) |

### Fake binaries (`<SB>/fakebin`, first on `PATH`)

* `steam` - `steam -silent` records the call and detaches a resident process whose process name (comm) is `steam`
  (so `pgrep -x steam` / `pkill -x steam` behave like a real client); `steam -applaunch <id>` records the call and exits.
* `xdg-open <url>` records the call and exits 0.
* Every call is appended as one line to `$PATCHOPS_BENCH_FAKESTEAM_LOG`; that file is part of the golden fs snapshot
  (so the port must invoke `steam -applaunch 311210`, `steam -silent`, `xdg-open steam://openurl/...` the same way, found via `PATH`).
  The port must *find* the client the same way the legacy code does (`PATH` lookup of `steam`, `pkill -x steam` to close it).
  How long it waits is not part of the contract, only the final state is, but those waits are what dominate the legacy numbers
  (see "Steam-dependent ops" below).

### Fixture (`bench/benchlib/sandbox.py` is the source of truth)

* Steam root `~/.steam/steam` with `config/config.vdf`, `steamapps/libraryfolders.vdf` listing library 0 (the root) and library 1 `<SB>/games/SteamLibrary`,
  `userdata/12345678/config/localconfig.vdf` (app `311210` has `LaunchOptions` = `WINEDLLOVERRIDES="dsound=n,b" %command%`; app `220` has `-novid`).
* Library 1 holds the game at `steamapps/common/Call of Duty Black Ops III/`: `BlackOps3.exe` (16 KiB of deterministic bytes, not a known build, so the
  executable is "unverified"), `d3dcompiler_46.dll`, `steam_api64.dll`, `installscript_311210.vdf`, `players/config.ini` (CRLF line endings, a lone
  `0xE9` byte inside a comment, duplicate `DrawFPS` line, blank lines before `MaxFPS`, a commented-out `// FOV`, `VsyncMode` sharing the `Vsync` prefix,
  an indented `SerializeRender`), `video/*.mkv` (4 files, one is `BO3_Global_Logo_LogoSequence.mkv`) + `keepme.bik`, `LPC/*.ff` (2 files).
  Workshop: item `2994481309` installed, `2942053577` subscribed only, `3667377161` unknown.
* `<SB>/games/alt/Call of Duty Black Ops III/BlackOpsIII.exe` (a second, config-less install), `<SB>/dumps/valid-dump/DUMP/{BlackOps3.exe,MicrosoftGame.config}`,
  `<SB>/dumps/invalid-dump/readme.md`.
* The game is **not** in the settings file at start: it is auto-detected through the Steam library list.
* Workload steps may apply named fixture mutations between ops (`install_fake_t7`, `install_fake_dxvk`, `install_fake_enhanced`, `place_mod_junk`,
  `place_fake_depot`, `seed_enhanced_hash`, `unseed_enhanced_hash`, `remove_video_dir`, `restore_video_dir`, `reset_fixture`). They simulate what the
  network installers would have produced, so uninstall/config paths can be exercised offline. They are done by the harness, not by the app.

### Golden snapshot semantics

After every step the harness snapshots `home/.local/share/PatchOpsIII`, `home/.steam`, `games`, `dumps`, `fakebin-log`
as `relative path -> {t: f|d|l, sha256, mode}`. Text files (valid UTF-8, no NUL, <4 MiB) are hashed after replacing `<SB>`, timestamps
`YYYY-MM-DD HH:MM:SS - ` -> `<TS> - `, the platform string in the log payload header, and sorting the (set-ordered) list in
`Dump source missing required files: a, b`. Everything else is hashed raw. `mode` is the permission bits (umask-derived `0644`/`0755` for files the app creates).
Chromium/Electron profile directories (`.config`, `.cache`) are not snapshotted. The response is compared after the same string normalisation, with the `logs` key
removed. Golden file `NNN-<label>.json` = `{op,args,response,fs_diff (vs previous step),fs_digest (whole tree after the step)}`.

## 3. Ops

Legacy route -> op name: strip `/api/`. GET ops take query parameters as `args`. "state" below means the full status object of section 4.
Every mutating op that returns `state` returns it **after** the mutation. `ok:false` responses of file-mutating ops return `{ "ok": false, "error": str }` without `state`
unless noted. Every op that "logs" appends to the log file (and the in-memory log ring) with the stated category and message; the exact messages are in the goldens.

| op | method | args | result (envelope `ok:true`) |
|---|---|---|---|
| `health` | GET | - | `{"ok":true,"version":"bench"}` |
| `status` | GET | - | state |
| `browse` | GET | `path?: string` | browse payload |
| `game-directory` | POST | `path: string (min 1)` | `{ok,state}` / `{ok:false,error}` |
| `launch` | POST | - | `{"ok":true}` always |
| `launch-options` | POST | `options: string = ""`, `preserve_fs_game: bool = false` | `{ok,state}` / `{ok:false,error:"Unsupported launch option.",state}` |
| `workshop-install` | POST | `profileId: string (min 1)` | `{ok,state}` / `{ok:false,error}` |
| `update-check` | POST | - | network; `{ok,update:{...},state}` / `{ok:false,error}` |
| `release-channel` | POST | `channel: string` | `{ok,state}` / `{ok:false,error}` |
| `config` | POST | `key: string (min 1)`, `value: string\|int\|float\|bool`, `comment: string = "Managed by PatchOpsIII"` | `{ok,state}` / `{ok:false,error}` |
| `vram-target` | POST | `limited: bool`, `target: int 75..100 = 75` | `{ok,state}` / `{ok:false,error}` |
| `presets/apply` | POST | `name: string` | `{ok,state}` / `{ok:false,error}` |
| `intro-skip` | POST | `enabled: bool` | `{ok,state}` / `{ok:false,error}` |
| `all-intros-skip` | POST | `enabled: bool` | `{ok,state}` / `{ok:false,error}` |
| `d3dcompiler` | POST | `enabled: bool` | `{ok,state}` / `{ok:false,error}` |
| `config-readonly` | POST | `enabled: bool` | `{ok,state}` / `{ok:false,error}` |
| `dxvk-config` | POST | `enableAsync: bool=true`, `gplAsyncCache: bool=true`, `numCompilerThreads: int 0..64=0`, `maxFrameRate: int 0..360=0`, `maxFrameLatency: int 0..16=1`, `tearFree: string="True"`, `hudEnabled: bool=false` | `{ok,state}` / `{ok:false,error}` |
| `dxvk-install` | POST | same body as `dxvk-config` | network (downloads DXVK-GPLAsync) |
| `dxvk-uninstall` | POST | - | `{ok,state}` |
| `t7-config` | POST | `gamertag?: string\|null`, `colorCode: string=""`, `networkPassword?: string\|null`, `friendsOnly?: bool\|null` | `{ok,state}` / `{ok:false,error}` |
| `t7-install` | POST | - | network |
| `t7-uninstall` | POST | - | `{ok,state}` |
| `enhanced-validate` | POST | `dumpSource: string (min 1)` | `{ok:true,valid:bool,message:str,state}` |
| `enhanced-install` | POST | `dumpSource: string (min 1)` | network |
| `enhanced-uninstall` | POST | - | `{ok,state}` / `{ok:false,error}` |
| `exe-swap/compatible` | POST | - | `{ok,state}` / `{ok:false,error,depotRequired?,depotCommand?,state?}` |
| `exe-swap/compatible-depot` | GET | - | `{ok:true,available:bool,state}` |
| `exe-swap/current` | POST | - | `{ok,state}` / `{ok:false,error}` |
| `exe-swap/enhanced` | POST | - | `{ok,state}` / `{ok:false,error}` |
| `logs/payload` | GET | - | `{ok:true,payload:str}` |
| `logs/clear` | POST | - | `{ok,state}` / `{ok:false,error}` |
| `mod-files/clear` | POST | - | `{ok,state}` |
| `reset-stock` | POST | - | `{ok,state}` |

Ops that need the network (`update-check`, `dxvk-install`, `t7-install`, `enhanced-install`) are in the optional `--network` group only; they are
part of the contract but have no golden. There is no environment override in the legacy code to point them at a local server (URLs are hard-coded
constants), so they hit the real GitHub/GitLab endpoints.

### Common rules

* "No game": every op marked *needs game dir* returns `{"ok": false, "error": "Game directory is not set."}` when no game dir resolves (settings -> libraries -> defaults).
* Config writes: `_write_config_values`: if `<game>/players/config.ini` is missing -> error text `config.ini not found at <path>`. The file is read as UTF-8 with
  invalid bytes **dropped** and universal newlines (`\r\n` -> `\n`), each `(key,value,comment)` is applied in order as: `replacement = f'{key} = "{value}" // {comment}'`;
  regex `^\s*<escaped key>\s*=.*$` (MULTILINE) -> if it matches, **all** matching lines are substituted (the leading `\s*` can swallow preceding blank lines; `value` is used as
  a `re.sub` replacement template, so `\n`, `\1`, `\g<0>` in values are interpreted and `\q` raises `bad escape \q at position N`), else the text is `rstrip()`ped and
  `replacement + "\n"` is appended on a new line. The file is rewritten as UTF-8 with `\n` newlines. `str(value)` formatting: `True`/`False` for bools,
  `1.0` for floats, `144` for ints; `vram-target` writes `target/100` as a Python float (`0.8`, `1.0`, `0.75`).
  A read-only file (non-root) makes the write fail with Python's `[Errno 13] Permission denied: '<path>'` text in `error`.
* Backups: renamed siblings with suffix `.patchops.bak` (legacy suffix `.bak` is also honoured when *restoring*; `.patchops.bak` wins when both exist).

### Op details

**browse** - `path` default = settings `game_dir` or `$HOME`; non-existent or non-directory -> `$HOME`; the chosen path is `resolve()`d.
Result: `path`, `parent` (null at `/`), `hasGameExecutable`, `roots` (`[{"label":"/","path":"/"}]`), `shortcuts` (`[{label,path}]`, in order: `Home`, `Desktop`, `Downloads`,
`Saved game folder` (settings), `Detected BO3 folder`, `Steam Library 1..n`; only existing directories, de-duplicated by resolved path, first label wins),
`entries` (direct sub-directories only: `{name,path,hasGameExecutable}`, sorted by (not hasGameExecutable, lowercase name), max 300).

**game-directory** - rejects (with log `Error: Selected directory is not a Black Ops III install: <path>`) unless the dir contains a game exe. Success sets `settings.game_dir` and logs
`Success: Game directory set to <path>`.

**launch** - runs `steam -applaunch 311210` (resolved through `PATH`; falls back to `xdg-open steam://rungameid/311210`). Always `{"ok":true}`; failure is only logged.

**launch-options** - allowed `options`: `""`, `+set fs_game offlinemp`, `+set fs_game 2994481309|2942053577|3667377161`; anything else -> `Unsupported launch option.` (Warning logged, `state` returned).
Otherwise (*Steam-dependent*): kill Steam (`pkill -x steam`; wait 5 s), back up `localconfig.vdf` to `<appdata>/backups/localconfig_backup.vdf` (copy2), merge
launch options into `UserLocalConfigStore/Software/Valve/Steam/apps/311210/LaunchOptions` (`vdf.dump(pretty=True)`: tab-indented, `"key"\t\t"value"` pairs, string escaping with `\"`),
start Steam (`steam -silent`, then poll up to 15 s until a `steam` process has been present for 2 s). Merge algorithm: pull `+set fs_game <token>` out of both the existing and requested strings;
keep the existing fs_game only if `preserve_fs_game` and no new one is requested; strip the exact tokens `WINEDLLOVERRIDES="dsound=n,b"` and `%command%` and remember whether either was present
(wine override present => `%command%` is forced present); result = `[WINEDLLOVERRIDES="dsound=n,b"] [%command%] <rest of existing, whitespace-normalised> <rest of requested> <fs_game>`, whitespace
collapsed. NB the *existing* remainder always precedes the requested one, i.e. requested options are appended, never replacing non-fs_game content. `ok` = whether the vdf was written (false e.g. when `localconfig.vdf` is missing; the Steam kill/start cycle still happens). If no Steam user directory exists, `apply_launch_options` raises `Steam user ID not found!` *before* touching Steam and the legacy route has no handler, so the HTTP answer is 500 (envelope `internal:`); `workshop-install` catches the same exception and returns `{ok:false,error:"Steam user ID not found!"}`.

**workshop-install** - unknown `profileId` (not `all_around|ultimate|reforged`) -> `{ok:false,error:"Select an installable Workshop mod."}`. Else applies that profile's launch option (same as launch-options, *Steam-dependent*, `preserve_fs_game=false`)
then runs `xdg-open steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=<workshop id>` (or `steam <url>` if no xdg-open).

**release-channel** - `channel` is trimmed + lower-cased; must be `stable|beta` else `{ok:false,error:"Release channel must be beta or stable."}`; stored in settings (`release_channel`).

**config** - see "Config writes". Without a game dir: `Game directory is not set.`; logs `Success: Set <key> to <value>.` (value printed with Python `str`).

**vram-target** - `limited` true: writes `VideoMemory = target/100` (comment `0.75 to 1`) and `StreamMinResident = "1"` (`0 or 1`); false: `VideoMemory = "1"`, `StreamMinResident = "0"`.

**presets/apply** - presets come from `presets.json` (`{name: {Key: [value, comment]}}`); all keys applied in one config write in file order; the special key `ReduceStutter` (not present in the shipped file)
renames `d3dcompiler_46.dll` to `.patchops.bak` when its value is `"1"`. Unknown preset name -> `error` is `str(KeyError)`, i.e. `'Nope'` including the quotes.

**intro-skip** - `enabled:true`: if `video/BO3_Global_Logo_LogoSequence.mkv` exists, rename it to `....mkv.patchops.bak`; always logs `Intro video skipped.`. `false`: if a backup exists rename it back; always logs `Intro video restored.`.
`qol.intro` = a backup exists (either suffix).

**all-intros-skip** - requires `video/` (else `Video directory was not found.`). `true`: every `*.mkv` -> `*.mkv.patchops.bak` unless that backup already exists. `false`: every `*.mkv.patchops.bak` and `*.mkv.bak` is renamed back unless the original name exists.
`qol.allIntros` = there are backup files and **no** `.mkv` files left.

**d3dcompiler** - `true`: rename `d3dcompiler_46.dll` -> `.patchops.bak` (log `Renamed d3dcompiler_46.dll to reduce stuttering.`); if already renamed -> log `Already using latest d3dcompiler.` (ok); if neither -> `d3dcompiler_46.dll was not found.`.
`false`: restore from backup or `No d3dcompiler backup was found.`.

**config-readonly** - requires an existing `config.ini` else `config.ini was not found.`; `true` -> `chmod 0400`, `false` -> `chmod 0600`. (Modes are in the goldens.)

**dxvk-config** - always (re)writes `<game>/dxvk.conf` even if DXVK is not installed: lines `dxvk.enableAsync=<true|false>`, `dxvk.gplAsyncCache=true` (only if `gplAsyncCache`), `dxvk.numCompilerThreads=N`,
`dxgi.maxFrameRate=N`, `dxgi.maxFrameLatency=N`, `dxvk.tearFree=<Auto|True|False>` (anything else becomes `Auto`), `dxvk.hud=fps,frametimes,gpuload` (only if `hudEnabled`); file ends with `\n`.
State `dxvk.settings` is parsed back from the file (a `dxvk.hud` line of any non-empty value means `hudEnabled`). `dxvk.installed` = both `dxgi.dll` and `d3d11.dll` exist.

**dxvk-uninstall** - removes `dxgi.dll`, `d3d11.dll`, `dxvk.conf` if present; logs a line per file and a final `DXVK-GPLAsync has been uninstalled.` or (nothing present) `DXVK-GPLAsync was not installed.` (Info). Always `ok`.

**t7-config** - requires `t7patch.conf` in the game dir for each field that is not null/omitted (checked per field, in the order gamertag, networkPassword, friendsOnly), else the specific error text in the goldens.
`gamertag` is trimmed; empty -> `Gamertag cannot be empty.`; > 20 chars -> `Gamertag cannot exceed 20 characters.`; written as `playername=<colorCode><name>`. `networkPassword` trimmed, written as `networkpassword=<pw>`
(empty clears). `friendsOnly` -> `isfriendsonly=1|0`. Missing keys are inserted at the top of the file; other lines are preserved verbatim; fields are applied one at a time (a failure in a later field leaves earlier ones applied).
`colorCode` alone changes nothing. State `t7.colorCode`/`plainName` split `gamertag` when it starts with `^` and is longer than 2 chars.

**t7-uninstall** - removes `t7patch.dll, t7patch.conf, discord_game_sdk.dll, dsound.dll, t7patchloader.dll, zbr2.dll` from the game dir, removes cached `BO3 Mod Files/{linux,Linux.Steamdeck.and.Manual.Windows.Install,T7Patch_extracted}` and `T7Patch.zip`,
then restores LPC: in `LPC/`, each `X.patchops.bak` (or `X.bak` if no `.patchops.bak`) replaces `X`. Always `ok`.

**enhanced-validate** - blank `dumpSource` (after trim) -> `{ok:true,valid:false,message:"Select a dump source first.",state}` **without** saving; otherwise stores `enhanced_dump_source` in settings, then validates
(directory or zip containing `BlackOps3.exe` and `MicrosoftGame.config`, optionally under `DUMP/`): `message` is `Source looks ready.` or `Source is missing required dump files.`.

**enhanced-uninstall** - *Steam-dependent* (Linux cleanup closes/reopens Steam, clears the `CompatToolMapping` entry for 311210 in `~/.steam/steam/config/config.vdf`, restores or cleans launch options, removes `compatibilitytools.d/BO3 Enhanced*`);
restores `.patchops.bak` files / removes the 4 Enhanced DLLs; writes `.patchops_exe_variant.json` `{"variant":"default"}` into the game dir.

**exe-swap/\*** - the three swap ops require the *active* executable to be trusted: SHA-256 equal to the latest Steam build, the compatible build, or `status=enhanced` (Enhanced files present, or the exe hash recorded in
`settings.enhanced_exe_hashes[<game_dir>]`). The benchmark fixture's exe is none of these, so the default outcome is the error
`Active executable hash <first 12 hex> does not match latest BuildID ..., compatible BuildID ..., or a PatchOpsIII-preserved Enhanced EXE.`; with Enhanced files/hash present the paths
`Enhanced executable is already active.` (ok), `No backup executable found. Cannot restore the current EXE.` and the `depotRequired` response (`error`, `depotRequired:true`, `depotCommand:"download_depot 311210 311211 9084453472036406216"`, `state`) are exercised.
`compatible-depot` -> `available` is true when `<library>/steamapps/content/app_311210/depot_311211/{BlackOps3.exe,installscript_311210.vdf}` exist (exe non-empty).

**logs/payload** - ``PatchOpsIII <version> - <System> <release> (<machine>) logs:\n```\n<log file contents, stripped, or "(no log entries found)">\n```` ``.

**logs/clear** - truncates the log file, clears the in-memory ring, then logs `Success: Logs cleared.` (so the file contains exactly that line afterwards).

**mod-files/clear** - creates `BO3 Mod Files` if needed and deletes everything inside it; logs `Success: Cleared mod files in <dir>`.

**reset-stock** - *Steam-dependent*. In order, each step in its own try/except that only logs a Warning: Enhanced uninstall if Enhanced files are present; restore the current-build exe from backup (or the plain-backup restore) - for untrusted exes this logs
`Executable reset step failed: ...`; T7 uninstall; DXVK uninstall; restore `d3dcompiler_46.dll` and intro videos; write config `SmoothFramerate=0`, `VideoMemory=1`, `StreamMinResident=0`, `MaxFrameLatency=1`, `SerializeRender=0`, `RestrictGraphicsOptions=1`
(comments `0 or 1`, `0.75 to 1`, `0 or 1`, `0 to 4`, `0 to 2`, `0 or 1`); clear Steam launch options (full Steam kill/start cycle); log `Reset to stock complete.`. Always `ok` once a game dir exists.

### Steam-dependent ops (timing note)

`launch-options`, `workshop-install`, `enhanced-uninstall`, `reset-stock` close and re-open the Steam client. In the legacy implementation `close_steam` always sleeps 5 s and `open_steam` polls
until the client has been seen for 2 s, so each cycle costs ~7.1 s regardless of hardware. The harness tags these steps and reports totals with and without them.

## 4. The `state` object

Returned by `status` and embedded in most mutating responses. (JSON key order is irrelevant. `logs` is ignored by the parity check.)

```
appVersion: str            platform: "Linux"          gameDir: str|null          gameDetected: bool       configExists: bool (config.ini non-empty)
steamUserId: str|null      logPath: str               presets: [str]             currentLaunchOptions: str|null
activeLaunchProfile: "default"|"offline"|"all_around"|"ultimate"|"reforged"|"custom"   (first active profile in that order; "custom" if options non-empty but no match; "default" if none)
releaseChannel: "stable"|"beta"
launchProfiles: [{id,label,option,active,installed,subscribed,state, path? (workshop only)}]  (default, offline, all_around, ultimate, reforged)
enhanced: {installed, detectedAt, acknowledgedAt, launchOptionsActive, dumpSource, filesInstalled, backupStatus}
exeSwap: {profile, modeLabel, patchLabel, displayLabel, state, activeBuildId, activeBuildDate, currentBuildId, currentBuildDate, compatibleBuildId, compatibleBuildDate, enhancedBuildId,
          enhancedBuildDate, executable, executableName, executableHash, trustedExecutable, integrityStatus, integrityMessage, backupAvailable, latestAvailable, compatibleAvailable,
          enhancedAvailable, compatibleActive, enhancedExeActive, enhancedActive}
t7: {installed, confExists, gamertag, plainName, colorCode, networkPassword, friendsOnly, mode: "Unknown"|"Default"|"Custom"|"Enhanced"}
dxvk: {installed, confExists, settings: {enableAsync, gplAsyncCache, numCompilerThreads, maxFrameRate, maxFrameLatency, tearFree, hudEnabled}}
qol: {d3dcompiler, intro, allIntros}
graphics: {maxFps, fov, displayMode, resolution, refreshRate, renderResolution, vsync, drawFps}
advanced: {smoothFramerate, unlockOptions, reduceCpu, maxFrameLatency, vramLimited, vramTarget, configReadonly}
maintenance: {modFilesDir, logPayload}
mods: {t7Patch, dxvk, enhanced}
logs: [{message, category, line}]   (last <=80 of a ring of 300)
```

Config-derived fields read the first occurrence of `^\s*Key\s*=\s*"([^"]+)"` (note: **non-empty** value required):
`maxFps=MaxFPS (int(float), default 165)`, `fov=FOV (80)`, `displayMode=FullScreenMode (1)`, `resolution=WindowSize ("1920x1080")`, `refreshRate=RefreshRate (float, 60)`, `renderResolution=ResolutionPercent (100)`,
`vsync=Vsync=="1" (default "1")`, `drawFps=DrawFPS=="1"`, `smoothFramerate=SmoothFramerate=="1"`, `unlockOptions=RestrictGraphicsOptions=="0" (default "1")`, `reduceCpu=SerializeRender=="2"`, `maxFrameLatency=MaxFrameLatency (1)`,
`vramLimited = not (VideoMemory=="1" and StreamMinResident=="0")` (defaults `"1"`/`"0"`), `vramTarget=int(float(VideoMemory)*100)` (default 0.75; note float truncation, e.g. `0.29`), `configReadonly = exists and not os.access(W_OK)`.
A value that does not parse as a number falls back to the default for that field.

`exeSwap.profile` is `"current"|"compatible"|"enhanced"|""`; `t7.mode` is `Unknown` without a game dir, `Enhanced` when the 4 Enhanced files exist, `Default` when the exe hash equals the latest Steam build or `.patchops_exe_variant.json` says `default`, else `Custom` (exe exists) / `Unknown`.

## 5. Behaviour the port should consciously decide on (also flagged `quirk` in the workload)

1. `config` values are `re.sub` templates (backslash escapes processed; `\q` is an error whose text is Python's).
2. `config.ini` is normalised to LF and loses invalid UTF-8 bytes on the first write; trailing whitespace is stripped on append; blank lines before a matched key are eaten.
3. `configReadonly` is `os.access`-based (never true for root); writes to a read-only file surface the OS error string.
4. `Dump source missing required files: a, b` list order is hash-seed dependent in legacy (the harness sorts it before hashing).
5. `status.logs` is filled asynchronously and can lag one entry behind the file (ignored by the parity check).
6. A missing Steam user dir makes `launch-options` raise (HTTP 500, envelope `internal:`) while `workshop-install` returns `{ok:false,error}` for the same condition.
7. `dxvk-config` creates `dxvk.conf` when DXVK is not installed; `intro-skip` always logs success even if nothing changed.
8. `t7-config` applies fields one by one; an early success is kept when a later field fails.

## 6. Examples

Generated from the golden files by `bench/tools/gen_ops_examples.py` (kept at the end of this file between the markers).

<!-- EXAMPLES:BEGIN -->
**`health`** `{}` -> envelope ok, app ok=True (golden step `health`)

```json
{"ok": true, "version": "bench"}
```

**`status`** `{}` -> envelope ok, app ok=None (golden step `status-initial`)

```json
{
 "activeLaunchProfile": "custom",
 "advanced": {
  "configReadonly": false,
  "maxFrameLatency": 1,
  "reduceCpu": false,
  "smoothFramerate": false,
  "unlockOptions": false,
  "vramLimited": true,
  "vramTarget": 75
 },
 "appVersion": "bench",
 "configExists": true,
 "currentLaunchOptions": "WINEDLLOVERRIDES=\"dsound=n,b\" %command%",
 "dxvk": {
  "confExists": false,
  "installed": false,
  "settings": {
   "enableAsync": true,
   "gplAsyncCache": true,
   "hudEnabled": false,
   "maxFrameLatency": 1,
   "maxFrameRate": 0,
   "numCompilerThreads": 0,
   "tearFree": "True"
  }
 },
 "enhanced": {
  "acknowledgedAt": null,
  "backupStatus": "Not created",
  "detectedAt": null,
  "dumpSource": "",
  "filesInstalled": 0,
  "installed": false,
  "launchOptionsActive": false
 },
 "exeSwap": {
  "activeBuildDate": "",
  "activeBuildId": "Unverified",
  "backupAvailable": false,
  "compatibleActive": false,
  "compatibleAvailable": false,
  "compatibleBuildDate": "Mar 3, 2023",
  "compatibleBuildId": "10650222",
  "currentBuildDate": "Sep 10, 2026",
  "currentBuildId": "24784313",
  "displayLabel": "Active executable hash 92ad6bbeeb6b does not match latest BuildID 24784313, compatible BuildID 10650222, or a PatchOpsIII-preserved Enhanced EXE. T7 installs will use T7 Patch Scroptss/T7Patch.",
  "enhancedActive": false,
  "enhancedAvailable": false,
  "enhancedBuildDate": "",
  "enhancedBuildId": "Enhanced",
  "enhancedExeActive": false,
  "executable": "<SB>/games/SteamLibrary/steamapps/common/Call of Duty Black Ops III/BlackOps3.exe",
  "executableHash": "92ad6bbeeb6b99115837261d607c4d6da8ed7c6c6c30dae42070a92e4f969818",
  "executableName": "BlackOps3.exe",
  "integrityMessage": "Active executable hash 92ad6bbeeb6b does not match latest BuildID 24784313, compatible BuildID 10650222, or a PatchOpsIII-preserved Enhanced EXE.",
  "integrityStatus": "unverified",
  "latestAvailable": false,
  "modeLabel": "Unknown",
  "patchLabel": "T7 Patch Scroptss/T7Patch",
  "profile": "",
  "state": "unverified",
  "trustedExecutable": false
 },
 "gameDetected": true,
 "gameDir": "<SB>/games/SteamLibrary/steamapps/common/Call of Duty Black Ops III",
 "graphics": {
  "displayMode": 1,
  "drawFps": false,
  "fov": 80,
  "maxFps": 165,
  "refreshRate": 59.94,
  "renderResolution": 100,
  "resolution": "1920x1080",
  "vsync": true
 },
 "launchProfiles": [
  {
   "active": false,
   "id": "default",
   "installed": true,
   "label": "Default (None)",
   "option": "",
   "state": "Ready",
   "subscribed": true
  },
  {
   "active": false,
   "id": "offline",
   "installed": true,
   "label": "Play Offline",
   "option": "+set fs_game offlinemp",
   "state": "Ready",
   "subscribed": true
  },
  {
   "active": false,
   "id": "all_around",
   "installed": true,
   "label": "All-around Enhancement Lite",
   "option": "+set fs_game 2994481309",
   "path": "<SB>/games/SteamLibrary/steamapps/workshop/content/311210/2994481309",
   "state": "Installed",
   "subscribed": true
  },
  {
   "active": false,
   "id": "ultimate",
   "installed": false,
   "label": "Ultimate Experience Mod",
   "option": "+set fs_game 2942053577",
   "path": null,
   "state": "Subscribed (not installed yet)",
   "subscribed": true
  },
  {
   "active": false,
   "id": "reforged",
   "installed": false,
   "label": "BO3 Reforged",
   "option": "+set fs_game 3667377161",
   "path": null,
   "state": "Not Subscribed",
   "subscribed": false
  }
 ],
 "logPath": "<SB>/home/.local/share/PatchOpsIII/PatchOpsIII.log",
 "maintenance": {
  "logPayload": "PatchOpsIII bench - <PLATFORM> logs:\n```\n(no log entries found)\n```",
  "modFilesDir": "<SB>/home/.local/share/PatchOpsIII/BO3 Mod Files"
 },
 "mods": {
  "dxvk": false,
  "enhanced": false,
  "t7Patch": false
 },
 "platform": "Linux",
 "presets": [
  "Smooth Preset",
  "Low-Latency Preset"
 ],
 "qol": {
  "allIntros": false,
  "d3dcompiler": false,
  "intro": false
 },
 "releaseChannel": "stable",
 "steamUserId": "12345678",
 "t7": {
  "colorCode": "",
  "confExists": false,
  "friendsOnly": false,
  "gamertag": "",
  "installed": false,
  "mode": "Custom",
  "networkPassword": "",
  "plainName": ""
 }
}
```

**`browse`** `{}` -> envelope ok, app ok=None (golden step `browse-default`)

```json
{"entries": [{"hasGameExecutable": false, "name": ".cache", "path": "<SB>/home/.cache"}, {"hasGameExecutable": false, "name": ".config", "path": "<SB>/home/.config"}, {"hasGameExecutable": false, "name": ".local", "path": "<SB>/home/.local"}, {"hasGameExecutable": false, "name": ".steam", "path": "<SB>/home/.steam"}, {"hasGameExecutable": false, "name": "Desktop", "path": "<SB>/home/Desktop"}, {"hasGameExecutable": false, "name": "Downloads", "path": "<SB>/home/Downloads"}], "hasGameExecutable": false, "parent": "<SB>", "path": "<SB>/home", "roots": [{"label": "/", "path": "/"}], "shortcuts": [{"label": "Home", "path": "<SB>/home"}, {"label": "Desktop", "path": "<SB>/home/Desktop"}, {"label": "Downloads", "path": "<SB>/home/Downloads"}, {"label": "Detected BO3 folder", "path": "<SB>/games/SteamLibrary/steamapps/common/Call of Duty Black Ops III"}, {"label": "Steam Library 1", "path": "<SB>/home/.steam/steam"}, {"label": "Steam Library 2", "path": "<SB>/games/SteamLibrary"}]}
```

**`game-directory`** `{"path": "{home}"}` -> envelope ok, app ok=False (golden step `game-directory-not-a-game`)

```json
{"error": "BlackOps3.exe or BlackOpsIII.exe was not found.", "ok": false}
```

* `game-directory` invalid args: `{"id":N,"ok":false,"error":"validation: ..."}`   (golden step `game-directory-missing-arg`, args `{}`)
**`game-directory`** `{"path": "{alt}"}` -> envelope ok, app ok=True (golden step `game-directory-alt-install`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`config`** `{"key": "MaxFPS", "value": 60}` -> envelope ok, app ok=False (golden step `config-without-config-ini`)

```json
{"error": "config.ini not found at <SB>/games/alt/Call of Duty Black Ops III/players/config.ini", "ok": false}
```

**`config-readonly`** `{"enabled": true}` -> envelope ok, app ok=False (golden step `config-readonly-without-config-ini`)

```json
{"error": "config.ini was not found.", "ok": false}
```

**`release-channel`** `{"channel": "beta"}` -> envelope ok, app ok=True (golden step `release-channel-beta`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`release-channel`** `{"channel": "nightly"}` -> envelope ok, app ok=False (golden step `release-channel-invalid`)

```json
{"error": "Release channel must be beta or stable.", "ok": false}
```

* `release-channel` invalid args: `{"id":N,"ok":false,"error":"validation: ..."}`   (golden step `release-channel-missing`, args `{}`)
**`config`** `{"comment": "Maximum FPS cap", "key": "MaxFPS", "value": 240}` -> envelope ok, app ok=True (golden step `config-MaxFPS`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

* `config` invalid args: `{"id":N,"ok":false,"error":"validation: ..."}`   (golden step `config-empty-key`, args `{"key": "", "value": 1}`)
**`vram-target`** `{"limited": true, "target": 80}` -> envelope ok, app ok=True (golden step `vram-target-limited-80`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

* `vram-target` invalid args: `{"id":N,"ok":false,"error":"validation: ..."}`   (golden step `vram-target-too-low`, args `{"limited": true, "target": 50}`)
**`presets/apply`** `{"name": "Smooth Preset"}` -> envelope ok, app ok=True (golden step `preset-smooth`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`presets/apply`** `{"name": "Nope"}` -> envelope ok, app ok=False (golden step `preset-unknown`)

```json
{"error": "'Nope'", "ok": false}
```

* `presets/apply` invalid args: `{"id":N,"ok":false,"error":"validation: ..."}`   (golden step `preset-missing-name`, args `{}`)
**`intro-skip`** `{"enabled": true}` -> envelope ok, app ok=True (golden step `intro-skip-on`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

* `intro-skip` invalid args: `{"id":N,"ok":false,"error":"validation: ..."}`   (golden step `intro-skip-bad-arg`, args `{"enabled": "maybe"}`)
**`all-intros-skip`** `{"enabled": true}` -> envelope ok, app ok=True (golden step `all-intros-on`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`all-intros-skip`** `{"enabled": true}` -> envelope ok, app ok=False (golden step `all-intros-no-video-dir`)

```json
{"error": "Video directory was not found.", "ok": false}
```

**`d3dcompiler`** `{"enabled": true}` -> envelope ok, app ok=True (golden step `d3dcompiler-on`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`d3dcompiler`** `{"enabled": false}` -> envelope ok, app ok=False (golden step `d3dcompiler-off-again`)

```json
{"error": "No d3dcompiler backup was found.", "ok": false}
```

**`config-readonly`** `{"enabled": true}` -> envelope ok, app ok=True (golden step `config-readonly-on`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`vram-target`** `{"limited": false}` -> envelope ok, app ok=False (golden step `vram-while-readonly`)

```json
{"error": "[Errno 13] Permission denied: '<SB>/games/SteamLibrary/steamapps/common/Call of Duty Black Ops III/players/config.ini'", "ok": false}
```

**`launch-options`** `{"options": "+set fs_game offlinemp", "preserve_fs_game": false}` -> envelope ok, app ok=True (golden step `launch-options-offline`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`launch-options`** `{"options": "+set fs_game 123"}` -> envelope ok, app ok=False (golden step `launch-options-unsupported`)

```json
{"error": "Unsupported launch option.", "ok": false, "state": "{...state object, see section 4...}"}
```

**`workshop-install`** `{"profileId": "nope"}` -> envelope ok, app ok=False (golden step `workshop-install-unknown`)

```json
{"error": "Select an installable Workshop mod.", "ok": false}
```

* `workshop-install` invalid args: `{"id":N,"ok":false,"error":"validation: ..."}`   (golden step `workshop-install-empty-id`, args `{"profileId": ""}`)
**`workshop-install`** `{"profileId": "ultimate"}` -> envelope ok, app ok=True (golden step `workshop-install-ultimate`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`launch`** `{}` -> envelope ok, app ok=True (golden step `launch`)

```json
{"ok": true}
```

**`t7-config`** `{"colorCode": "^5", "gamertag": "Bench"}` -> envelope ok, app ok=False (golden step `t7-config-without-conf`)

```json
{"error": "t7patch.conf was not found. Install T7 Patch before updating the gamertag.", "ok": false}
```

**`t7-config`** `{"colorCode": "^5", "gamertag": "Bench"}` -> envelope ok, app ok=True (golden step `t7-gamertag`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`t7-uninstall`** `{}` -> envelope ok, app ok=True (golden step `t7-uninstall`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`dxvk-config`** `{}` -> envelope ok, app ok=True (golden step `dxvk-config-without-install`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`dxvk-uninstall`** `{}` -> envelope ok, app ok=True (golden step `dxvk-uninstall-conf-only`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

* `dxvk-config` invalid args: `{"id":N,"ok":false,"error":"validation: ..."}`   (golden step `dxvk-config-threads-too-high`, args `{"numCompilerThreads": 65}`)
**`enhanced-validate`** `{"dumpSource": "   "}` -> envelope ok, app ok=True (golden step `enhanced-validate-empty`)

```json
{"message": "Select a dump source first.", "ok": true, "state": "{...state object, see section 4...}", "valid": false}
```

* `enhanced-validate` invalid args: `{"id":N,"ok":false,"error":"validation: ..."}`   (golden step `enhanced-validate-missing-arg`, args `{}`)
**`enhanced-install`** `{"dumpSource": "{sb}/nope"}` -> envelope ok, app ok=False (golden step `enhanced-install-bad-source`)

```json
{"error": "The selected dump source does not exist.", "ok": false}
```

**`exe-swap/compatible`** `{}` -> envelope ok, app ok=False (golden step `exe-swap-compatible-unverified`)

```json
{"error": "Active executable hash 92ad6bbeeb6b does not match latest BuildID 24784313, compatible BuildID 10650222, or a PatchOpsIII-preserved Enhanced EXE.", "ok": false}
```

**`exe-swap/compatible-depot`** `{}` -> envelope ok, app ok=True (golden step `exe-swap-depot-absent`)

```json
{"available": false, "ok": true, "state": "{...state object, see section 4...}"}
```

**`exe-swap/current`** `{}` -> envelope ok, app ok=False (golden step `exe-swap-current-unverified`)

```json
{"error": "Active executable hash 92ad6bbeeb6b does not match latest BuildID 24784313, compatible BuildID 10650222, or a PatchOpsIII-preserved Enhanced EXE.", "ok": false}
```

**`exe-swap/enhanced`** `{}` -> envelope ok, app ok=False (golden step `exe-swap-enhanced-unverified`)

```json
{"error": "Active executable hash 92ad6bbeeb6b does not match latest BuildID 24784313, compatible BuildID 10650222, or a PatchOpsIII-preserved Enhanced EXE.", "ok": false}
```

**`exe-swap/enhanced`** `{}` -> envelope ok, app ok=True (golden step `exe-swap-enhanced-already-active`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`enhanced-uninstall`** `{}` -> envelope ok, app ok=True (golden step `enhanced-uninstall`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`logs/payload`** `{}` -> envelope ok, app ok=True (golden step `logs-payload`)

```json
{"ok": true, "payload": "PatchOpsIII bench - <PLATFORM> logs:\n```\n<TS> - Error: Selected directory is not a Black Ops III install: <SB>/home\n<TS> - Success: Game directory set to <SB>/games/alt/Call of Duty Black Ops III\n<TS> - Error: config.ini not found at <SB>/games/alt/Call of Duty Black Ops III/players/config.ini\n<TS> - Success: Game directory set to <SB>/games/SteamLibrary/steamapps/common/Call of Duty Black Ops III\n<TS> - Success: Release channel set to Beta.\n<TS> - Success: Release channel set to Stable.\n<TS> - Success: Set MaxFPS to 240.\n<TS> - Success: Set FOV to 100.\n<TS> - Success: Set FullScreenMode to 2.\n<TS> - Success: Set WindowSize to 2560x1440.\n<TS> - Success: Set RefreshRate to 144.\n<TS> - Success: Set ResolutionPercent to 125.\n<TS> - Success: Set Vsync to 0.\n<TS> - Success: Set DrawFPS to 1.\n<TS> - Success: Set SmoothFramerate to 1.\n<TS> - Success: Set RestrictGraphicsOptions to 0.\n<TS> - Success: Set SerializeRender to 2.\n<TS> - Success: Set MaxFrameLatency to 3.\n<TS> - Success: Set BenchNewKey to abc.\n<TS> - Success: Set BenchBool to True.\n<TS> - Success: Set BenchFloat to 1.0.\n<TS> - Success: Set BenchDefault to 7.\n<TS> - Success: Set BenchEsc to a\\nb.\n<TS> - Success: Set BenchEsc to x\\ny.\n<TS> - Error: bad escape \\q at position 12\n<TS> - Success: Limited VRAM usage set to 80%.\n<TS> - Success: Limited VRAM usage set to 100%.\n<TS> - Success: Limited VRAM usage set to 75%.\n<TS> - Success: Enabled full VRAM usage.\n<TS> - Success: Applied preset 'Smooth Preset'.\n<TS> - Success: Applied preset 'Low-Latency Preset'.\n<TS> - Error: Failed to apply preset: 'Nope'\n<TS> - Success: Intro video skipped.\n<TS> - Success: Intro video skipped.\n<TS> - Success: Intro video restored.\n<TS> - Success: Intro video restored.\n<TS> - Success: Intro video skipped.\n<TS> - Success: All intro videos skipped.\n<TS> - Success: Intro videos were already skipped.\n<TS> - Success: Intro videos restored.\n<TS> - Success: No intro video backups were found.\n<TS> - Success: Intro video skipped.\n<TS> - Success: Renamed d3dcompiler_46.dll to reduce stuttering.\n<TS> - Success: Already using latest d3dcompiler.\n<TS> - Success: Restored d3dcompiler_46.dll.\n<TS> - Success: config.ini set to read-only.\n<TS> - Error: [Errno 13] Permission denied: '<SB>/games/SteamLibrary/steamapps/common/Call of Duty Black Ops III/players/config.ini'\n<TS> - Error: Failed to apply preset: [Errno 13] Permission denied: '<SB>/games/SteamLibrary/steamapps/common/Call of Duty Black Ops III/players/config.ini'\n<TS> - Error: Failed to update VRAM target: [Errno 13] Permission denied: '<SB>/games/SteamLibrary/steamapps/common/Call of Duty Black Ops III/players/config.ini'\n<TS> - Success: config.ini set to writable.\n<TS> - Success: Set MaxFPS to 165.\n<TS> - Info: Steam was not running.\n<TS> - Success: Config backup created at <SB>/home/.local/share/PatchOpsIII/backups/localconfig_backup.vdf\n<TS> - Info: Setting launch options to: WINEDLLOVERRIDES=\"dsound=n,b\" %command% +set fs_game offlinemp\n<TS> - Info: Setting launch options...\n<TS> - Info: Opening Steam...\n<TS> - Success: Steam launched successfully\n<TS> - Warning: Unsupported launch option.\n<TS> - Success: Config backup created at <SB>/home/.local/share/PatchOpsIII/backups/localconfig_backup.vdf\n<TS> - Info: Setting launch options to: WINEDLLOVERRIDES=\"dsound=n,b\" %command% +set fs_game 2994481309\n<TS> - Info: Setting launch options...\n<TS> - Info: Opening Steam...\n<TS> - Success: Steam launched successfully\n<TS> - Success: Config backup created at <SB>/home/.local/share/PatchOpsIII/backups/localconfig_backup.vdf\n<TS> - Info: Setting launch options to: WINEDLLOVERRIDES=\"dsound=n,b\" %command% +set fs_game 2994481309\n<TS> - Info: Setting launch options...\n<TS> - Info: Opening Steam...\n<TS> - Success: Steam launched successfully\n<TS> - Success: Config backup created at <SB>/home/.local/share/PatchOpsIII/backups/localconfig_backup.vdf\n<TS> - Info: Setting launch options to: WINEDLLOVERRIDES=\"dsound=n,b\" %command%\n<TS> - Info: Setting launch options...\n<TS> - Info: Opening Steam...\n<TS> - Success: Steam launched successfully\n<TS> - Success: Config backup created at <SB>/home/.local/share/PatchOpsIII/backups/localconfig_backup.vdf\n<TS> - Info: Setting launch options to: WINEDLLOVERRIDES=\"dsound=n,b\" %command% +set fs_game 2942053577\n<TS> - Info: Setting launch options...\n<TS> - Info: Opening Steam...\n<TS> - Success: Steam launched successfully\n<TS> - Info: Opened Ultimate Experience Mod Workshop page in Steam.\n<TS> - Success: Launched Black Ops III via Steam (AppID: 311210) using steam -applaunch\n<TS> - Success: Updated 'playername' in t7patch.conf to '^5Bench'.\n<TS> - Success: Updated 'playername' in t7patch.conf to 'Spaced Name'.\n<TS> - Success: Updated network password in t7patch.conf.\n<TS> - Success: Updated network password in t7patch.conf.\n<TS> - Success: Cleared network password in t7patch.conf.\n<TS> - Success: Updated 'isfriendsonly' in t7patch.conf to On.\n<TS> - Success: Updated 'isfriendsonly' in t7patch.conf to Off.\n<TS> - Success: Updated 'playername' in t7patch.conf to '^2Combo'.\n<TS> - Success: Updated network password in t7patch.conf.\n<TS> - Success: Updated 'isfriendsonly' in t7patch.conf to On.\n<TS> - Success: Uninstalled T7 Patch files from the game directory.\n<TS> - Success: Removed cached T7 Patch files.\n<TS> - Success: Restored 2 LPC backup files\n<TS> - Success: T7 Patch has been completely uninstalled.\n<TS> - Success: T7 Patch has been completely uninstalled.\n<TS> - Success: Updated dxvk.conf from DXVK settings.\n<TS> - Success: Removed dxvk.conf.\n<TS> - Success: DXVK-GPLAsync has been uninstalled.\n<TS> - Success: Updated dxvk.conf from DXVK settings.\n<TS> - Success: Updated dxvk.conf from DXVK settings.\n<TS> - Success: Updated dxvk.conf from DXVK settings.\n<TS> - Success: Removed dxgi.dll.\n<TS> - Success: Removed d3d11.dll.\n<TS> - Success: Removed dxvk.conf.\n<TS> - Success: DXVK-GPLAsync has been uninstalled.\n<TS> - Info: DXVK-GPLAsync was not installed.\n<TS> - Error: Dump source missing required files: BlackOps3.exe, MicrosoftGame.config\n<TS> - Error: Dump source not found: <SB>/nope\n<TS> - Error: BO3 Enhanced install failed: The selected dump source does not exist.\n<TS> - Error: Dump source missing required files: BlackOps3.exe, MicrosoftGame.config\n<TS> - Error: BO3 Enhanced install failed: The dump source is missing required files.\n<TS> - Error: Compatible EXE swap failed: Active executable hash 92ad6bbeeb6b does not match latest BuildID 24784313, compatible BuildID 10650222, or a PatchOpsIII-preserved Enhanced EXE.\n<TS> - Error: Current EXE restore failed: Active executable hash 92ad6bbeeb6b does not match latest BuildID 24784313, compatible BuildID 10650222, or a PatchOpsIII-preserved Enhanced EXE.\n<TS> - Error: Enhanced EXE restore failed: Active executable hash 92ad6bbeeb6b does not match latest BuildID 24784313, compatible BuildID 10650222, or a PatchOpsIII-preserved Enhanced EXE.\n<TS> - Info: Compatible Steam depot was not found. Copy the depot command, open Steam Console, run it, then continue once the download finishes.\n<TS> - Error: Current EXE restore failed: No backup executable found. Cannot restore the current EXE.\n<TS> - Info: Enhanced executable is already active.\n<TS> - Success: Dump files uninstalled.\n<TS> - Success: Uninstall completed. Restored 0 files from backup; removed 4 files.\n<TS> - Success: Config backup created at <SB>/home/.local/share/PatchOpsIII/backups/steam_config_backup.vdf\n<TS> - Success: Cleared compatibility mapping for AppID 311210.\n<TS> - Info: No BO3 Enhanced launch override found; leaving launch options unchanged.\n<TS> - Info: Compatibility tool 'BO3 Enhanced' was already absent.\n<TS> - Info: Setting launch options...\n<TS> - Info: Opening Steam...\n<TS> - Success: Steam launched successfully\n<TS> - Success: Linux BO3 Enhanced cleanup complete: mapping cleared, launch options reset, and tool removed.\n<TS> - Success: Uninstalled BO3 Enhanced successfully.\n<TS> - Success: Dump files uninstalled.\n<TS> - Warning: No BO3 Enhanced files were removed or restored (none tracked).\n<TS> - Success: Config backup created at <SB>/home/.local/share/PatchOpsIII/backups/steam_config_backup.vdf\n<TS> - Success: Cleared compatibility mapping for AppID 311210.\n<TS> - Info: No BO3 Enhanced launch override found; leaving launch options unchanged.\n<TS> - Info: Compatibility tool 'BO3 Enhanced' was already absent.\n<TS> - Info: Setting launch options...\n<TS> - Info: Opening Steam...\n<TS> - Success: Steam launched successfully\n<TS> - Success: Linux BO3 Enhanced cleanup complete: mapping cleared, launch options reset, and tool removed.\n<TS> - Success: Uninstalled BO3 Enhanced successfully.\n<TS> - Error: Compatible EXE swap failed: Active executable hash 92ad6bbeeb6b does not match latest BuildID 24784313, compatible BuildID 10650222, or a PatchOpsIII-preserved Enhanced EXE.\n<TS> - Info: Compatible Steam depot was not found. Copy the depot command, open Steam Console, run it, then continue once the download finishes.\n<TS> - Error: Current EXE restore failed: No backup executable found. Cannot restore the current EXE.\n<TS> - Info: Enhanced executable is already active.\n```"}
```

**`logs/clear`** `{}` -> envelope ok, app ok=True (golden step `logs-clear`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`mod-files/clear`** `{}` -> envelope ok, app ok=True (golden step `mod-files-clear`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

**`reset-stock`** `{}` -> envelope ok, app ok=True (golden step `reset-stock`)

```json
{"ok": true, "state": "{...state object, see section 4...}"}
```

<!-- EXAMPLES:END -->
