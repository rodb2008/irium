# PoAW-X Phase 24C — storage isolation fail-closed hardening

**Safety-hardening phase (code + tests). No live nodes were launched. `~/.irium` was never
modified.** Local-only; not pushed; remote branch absent; `main` untouched. This fixes the
root cause of the Phase 24B incident so live testing can be reconsidered later.

## Old behavior (dangerous)

`storage::configured_dir(var)` returned `None` in **two** different situations:
- the env var was **UNSET** (legitimate → caller uses the default), and
- the env var was **SET but invalid** (not resolvable under `$HOME`, e.g. `/tmp/...`).

Callers used `configured_dir("X").unwrap_or_else(|| default)`, so a **set-but-invalid** path
**silently fell back to the default `~/.irium`**. On a host that also runs mainnet, `~/.irium`
is the mainnet live block store — and a devnet node then quarantined mainnet blocks (Phase
24B). Separately, `iriumd` had **no exiting `--help`**: any invocation booted a node and
initialized storage.

## New behavior (fail-closed)

- `storage::resolve_configured_dir(var) -> Result<Option<PathBuf>, String>` distinguishes:
  - `Ok(None)` — var **UNSET** (default permitted),
  - `Ok(Some(path))` — set to a path that resolves **under `$HOME`**,
  - `Err(msg)` — set to a path that does **not** resolve under `$HOME` (names the var + path).
- `storage::configured_dir` now **fails closed** (`eprintln` + `exit 78`) on an explicit
  invalid path — it never silently falls back to `~/.irium`. UNSET vars still use the default,
  so **mainnet (env unset or `$HOME`-rooted) is unaffected**.
- `storage::validate_storage_env()` checks `IRIUM_DATA_DIR` / `IRIUM_BLOCKS_DIR` /
  `IRIUM_STATE_DIR` / `IRIUM_BOOTSTRAP_DIR` up front and returns a clear `Err` naming the first
  offender.
- `iriumd` `main()`:
  - `--help` / `-h` → prints usage and **exits 0 before any storage init or node start**.
  - calls `validate_storage_env()` right after a config-derived `IRIUM_DATA_DIR` is applied and
    **before** `ensure_runtime_dirs()`; on `Err` it prints `[fatal][storage] …` and exits 78.

### Proven live (no `~/.irium` touch)
- `iriumd --help` → usage, **exit 0**, **0** storage/node banner lines.
- `IRIUM_BLOCKS_DIR=/tmp/... iriumd` → `[fatal][storage] IRIUM_BLOCKS_DIR is set to "/tmp/..."
  which does not resolve under HOME … refusing to fall back to the default ~/.irium`, **exit
  78**, node does not boot.

## Exact safe launch requirements (for any future live rehearsal)

- `/tmp` is **not** accepted by `storage::configured_dir()` — storage dirs must resolve under
  `$HOME`.
- Set **explicit `$HOME`-rooted** `IRIUM_DATA_DIR`, `IRIUM_BLOCKS_DIR`, and `IRIUM_STATE_DIR`
  (e.g. `/home/irium/irium-p24X-nodeA/{blocks,state}`); an invalid explicit path now causes
  the node to **exit (78)** instead of falling back.
- Still verify the printed `Using blocks dir:` / `Using state dir:` lines are the isolated
  paths before continuing; abort if either resolves to `~/.irium`.
- Prefer running devnet rehearsals on a host that does **not** also run mainnet.

## Tests

- `storage::phase24c_resolve_configured_dir_unset_invalid_valid` — UNSET → `Ok(None)`; `/tmp`
  → `Err` (names the var); `$HOME`-rooted → `Ok(Some(exact))`, for DATA/BLOCKS/STATE.
- `storage::phase24c_validate_storage_env_fail_closed` — all unset → `Ok`; `/tmp` → `Err`
  naming it; `$HOME`-rooted → `Ok`.
- `storage::phase24c_default_preserved_when_unset` — env unset → `blocks_dir()`/`state_dir()`
  resolve to the `~/.irium` default (path computation only; no fs touch).
- `iriumd::phase24c_args_request_help` — `--help`/`-h` detected; `--version`/none not.

(All tests use path logic / temporary `$HOME`-rooted names; **none touch the real
`~/.irium`**.)

## Status

- The Phase 24B incident **root cause is fixed** (silent fallback eliminated; help exits
  safely).
- **Phase 24B remains PAUSED with no validation claim.** A future live rehearsal must use the
  safe launch requirements above and verify the printed storage paths before continuing.
- This is internal hardening; it does **not** replace an external audit or a public testnet,
  and makes **no** mainnet-ready claim. Mainnet hard-off and chain difficulty/LWMA-144 are
  unchanged.
