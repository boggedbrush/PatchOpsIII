# Latest parity results

test result: FAILED. 17 passed; 11 failed; 3 ignored; 0 measured; 0 filtered out; finished in 10.53s

135 leaf mismatches across 11 failing cases. Every failure is described below; [results.jsonl](results.jsonl) contains every expected/actual value, including intermediate checkpoints and derivative size/hash mismatches. [results.provenance.json](results.provenance.json) records source snapshots and the executed test binary hash.

The core is being edited concurrently. These results describe the compiled test run; source hashes were sampled before launch and when writing this report, and the provenance file lists any changes between those samples. Later core edits require another run. Python module hashes are recorded in each golden and match local main.

Generator verification: `python3 scripts/gen_parity_fixtures.py --check` passed for all 31 cases. This environment used the direct-endpoint fallback, not FastAPI request validation.

| Failing case | Differences | Expected vs actual | Likely cause / location |
| --- | ---: | --- | --- |
| `config_append_and_crlf` | 6 | Python rewrites CRLF to LF; Rust retains CRLF. Values, comments and file modes now match. | `native/core/src/app.rs:169` — write_config_values deliberately retains the detected line ending. |
| `config_readonly_roundtrip` | 17 | Python uses 0400/0600; Rust uses 0444/0644. Rejected write expects the Python Permission denied message; Rust returns its own read-only explanation and logs it. | `native/core/src/app.rs:331`; `native/core/src/app.rs:169` — preserve-mode chmod and preflight rejection differ from Python chmod/write. |
| `dxvk_config_write` | 15 | Python returns ok=true and updates async, threads, FPS, latency, tear-free and HUD. Rust returns ok=false with “DXVK is not managed by PatchOpsIII…” and leaves dxvk.conf unchanged. | `native/core/src/dxvk.rs:175` — configure_managed requires an ownership manifest or recognized legacy DLL digests. This is an explicit safety policy difference; fake DLLs cannot satisfy legacy digest pins. |
| `game_directory_validation_invalid` | 6 | The error response now matches. Python logs “Selected directory is not a Black Ops III install: $ROOT/not-a-game”; Rust logs “BlackOps3.exe or BlackOpsIII.exe was not found.” | `native/core/src/app.rs:44`; `native/core/src/engine.rs:40` — generic dispatch error logging replaces the endpoint-specific message. |
| `launch_options_apply` | 31 | Current launch options and active profile match. Python updates the rolling localconfig backup on the second apply; Rust skips a no-op write and keeps oldmod in the backup. Rust also creates localconfig.vdf.patchops.bak, uses a tab between VDF key/value instead of a space, changes modes, and emits different logs. | `native/core/src/steam.rs:408`; `native/core/src/steam.rs:388`; `native/core/src/steam.rs:263` — no-op short circuit, persistent original backup and serializer. Some process-log differences depend on Python close/open being mocked; persistence differences do not. |
| `launch_options_unsupported` | 7 | Python returns ok=false/error plus state and logs Warning. Rust returns ok=false/error without state and logs Error. | `native/core/src/engine.rs:40` — generic mutation error conversion loses the endpoint-specific state and severity. |
| `qol_toggle_roundtrip` | 41 | Files and QoL flags match. Repeated compiler enable expects “Already using latest d3dcompiler.”; Rust says it renamed the DLL. Repeated all-intro enable expects “Intro videos were already skipped.”; Rust says “All intro videos skipped.” | `native/core/src/app.rs:402`; `native/core/src/app.rs:435` — success log selection ignores whether a file changed. |
| `status_enhanced_installed` | 4 | Legacy Enhanced state expects filesInstalled=4, backupStatus=Created and the two seeded timestamps. Rust reports 0/Not created/null/null, although installed=true matches. | `native/core/src/enhanced.rs:146`; `native/core/src/enhanced.rs:201` — legacy Python state has no game_directory; status discards unbound ownership metadata. Requires safe legacy migration or a documented intentional behavior change. |
| `t7_config_invalid_name` | 3 | Empty/overlong-name error strings now match, including periods. Python emits no log/file for validation failure; Rust creates an Error log file. | `native/core/src/engine.rs:40` — generic dispatch error logging adds validation side effects. |
| `t7_config_missing` | 2 | The missing-gamertag-config error text now matches. Python emits no log/file; Rust creates PatchOpsIII.log with an Error entry. | `native/core/src/engine.rs:40` — generic dispatch error logging adds a validation side effect. |
| `vram_target_roundtrip` | 3 | At 100%, Python writes VideoMemory="1.0"; Rust writes "1". Response values, final config text and permissions match. The intermediate checkpoint catches the byte difference. | `native/core/src/app.rs:292` — set_vram_target trims trailing zeros from the decimal value. |

## Passing cases

`config_graphics_advanced_write`, `config_missing_file`, `exe_preserved_enhanced_detection`, `exe_untrusted_backup_detection`, `game_directory_validation_valid`, `qol_restore_legacy`, `qol_toggles`, `status_alternate_executable`, `status_config_variants`, `status_dxvk_installed`, `status_empty_config`, `status_no_game`, `status_steam_secondary_library`, `status_t7_installed`, `status_unverified_base`, `t7_config_clear_and_append`, `t7_config_write`.

Earlier failures in shared status JSON, config comments, settings serialization, preserved Enhanced T7 mode, T7 config logging/staging, and existing-file atomic permissions now pass after concurrent core fixes. No goldens were changed to accept Rust behavior.

## Explicit adapter gaps

- September 2026 current and compatible Steam EXE cases require an injectable hash provider. Their physical marker bytes cannot have the game executable digests.
- Versioned/renamed T7 release asset discovery requires a public pure selector or injectable HTTP transport. The private installer otherwise downloads archives.

Functional response gaps (missing state, rejected DXVK config, legacy Enhanced metadata) merit implementation review. Byte/permission/log differences are also strict parity failures; some ownership/backup behavior is deliberately stricter in Rust. Steam process log comparisons are limited by Python’s disabled close/open hooks. No process lifecycle or network/download integration was validated.

The test agent edited only the generator and parity test paths. No commits, pushes, or stashes were made.
