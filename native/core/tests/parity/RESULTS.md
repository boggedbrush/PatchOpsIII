# Latest parity results

Linux isolated run: **31 passed, 0 failed, 0 ignored; zero leaf mismatches**.
`results.jsonl` is empty; `results.provenance.json` records the exact source
hashes, compiled test binary SHA-256, and reviewed DXVK ownership allowlist.
No Python golden or generator change was needed.

Validation: 100 core unit tests and 3 GPUI tests passed in the final workspace run, as did workspace tests,
format check, workspace/all-target Clippy with `-D warnings`, and
`python3 scripts/gen_parity_fixtures.py --check` (all 31 deterministic goldens).
The test target is Linux-only; other platforms compile it as an empty target.
Ubuntu CI requires `bubblewrap`, already in the native workflow's apt packages.

| Case / group | Final behavior |
| --- | --- |
| `config_append_and_crlf` | Text-mode parity: CRLF input is rewritten with LF. |
| `config_readonly_roundtrip` | chmod uses 0400/0600; rejected write returns/logs the Python Permission denied message and leaves the file untouched. |
| `launch_options_apply` | Space between VDF string key/value, rolling backup on each apply including no-op, source permissions retained, no localconfig sibling backup, Python operation log text/order. Lifecycle hooks are mocked on both runners. |
| `launch_options_unsupported`, invalid game directory, invalid/missing T7 config | Existing endpoint-specific envelopes/severity and no validation log side effects pass unchanged. |
| QoL toggles and VRAM | Existing repeated-enable “already” messages and `1.0` limited-VRAM formatting pass unchanged. |
| `dxvk_config_write` | Verified legacy DLL pair is adopted before configuring; exact manifest and original-config backup are asserted by the narrow allowlist. Foreign/mixed/incomplete pairs remain refused. Synthetic DLL recognition uses a fixture-only injected hash provider. |
| `status_enhanced_installed` | Matching legacy records bind to one canonical game directory in memory per AppState, preserving count/backup status/timestamps and the exact legacy JSON. Missing/unsafe/mismatched files cannot bind; binding never grants destructive ownership. |
| September 2026 and compatible EXEs | Both formerly ignored cases now execute through the status-only hash provider. Compatible T7 status uses Python's `Custom` label. EXE mutation hashing remains physical. |
| Versioned/renamed T7 release discovery | Formerly ignored case executes via a public pure selector taking release JSON, using the installer's selection algorithm and the Engine's discovery log wrapper. |

The allowlist accepts only the two DXVK ownership directories, exact manifest,
and original config backup; all Python responses, existing files, modes, and
logs remain strict. Read `README.md` for exact adoption, normalization, and
binding rules. Historical `rustIgnore` metadata in immutable goldens is no
longer acted on.

Limits: the Python oracle uses direct endpoint calls with import/model shims in
this environment; it does not verify FastAPI validation or lifecycle hooks.
Steam lifecycle is disabled in both fixture runners. No network/download or
real-game operation is performed. GitHub-hosted Ubuntu/Windows jobs were not
executed here. A durable Enhanced status binding across app restarts needs an
additional reviewed filesystem difference; this implementation deliberately
keeps the Python golden JSON intact and uses an AppState-local binding.
