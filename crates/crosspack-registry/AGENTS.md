# CROSSPACK-REGISTRY KNOWLEDGE BASE

## OVERVIEW
Registry source state, snapshot lifecycle, and manifest signature enforcement live in `src/lib.rs`.

## CORE SURFACES
- `RegistrySourceStore`: owns `sources.toml`, source CRUD, ordering, and update orchestration.
- `RegistrySourceRecord`: source contract (`name`, `kind`, `location`, `fingerprint_sha256`, `enabled`, `priority`).
- `update_sources` + `finalize_staged_source_update`: stage, validate, atomically swap cache, then write `snapshot.json`.
- `RegistrySourceSnapshotState`: per-source cache health (`None`, `Ready`, `Error` with reason code).
- `RegistryIndex`: reads `index/<package>/*.toml`, verifies `*.toml.sig`, parses manifests.
- `ConfiguredRegistryIndex`: loads enabled sources with ready snapshots only; resolves in source priority order.

## TRUST MODEL
- Trust root is per-source `registry.pub`; expected key fingerprint is pinned in `sources.toml`.
- Updates fail closed if computed key fingerprint mismatches `fingerprint_sha256`.
- Metadata validity requires both signed manifest bytes and parseable TOML payload.
- Signature verification uses `verify_ed25519_signature_hex` from `crosspack-security`.
- Layout is mandatory: staged source must contain `registry.pub` and `index/` before acceptance.
- Snapshot readiness gate is explicit: only `snapshot.json` with `status == "ready"` is eligible for configured reads.

## CHANGE IMPACT
- Changing `RegistrySourceRecord` fields or serde aliases impacts persisted `sources.toml` compatibility.
- Touching `parse_source_state_file`/`state_file_version` affects upgrade path for legacy state files.
- Altering snapshot ID derivation (`git:` truncation or filesystem hash input) changes update status semantics.
- Modifying `source_has_ready_snapshot` or `read_snapshot_state` can hide or surface sources unexpectedly.
- Any change in `RegistryIndex::package_versions` error strings/signature flow affects CLI failure diagnostics.
- Reordering source selection in `ConfiguredRegistryIndex` changes package resolution precedence.

## ANTI-PATTERNS (REGISTRY)
- Do not bypass fingerprint check even for local/filesystem sources.
- Do not load manifests without verifying matching `.toml.sig` against `registry.pub`.
- Do not accept partial cache replacement; preserve backup/restore behavior on write failures.
- Do not treat unreadable/invalid snapshots as ready.
- Do not loosen source-name or fingerprint validators without migration strategy.
- Do not silently continue on per-source update failures; keep explicit failed status/error payloads.

## QUICK CHECKS
- Run crate tests: `rustup run stable cargo test -p crosspack-registry`.
- Run focused state/snapshot tests: `rustup run stable cargo test -p crosspack-registry source_store_list_sources_with_snapshot_state`.
- Run signature policy path tests: `rustup run stable cargo test -p crosspack-registry package_versions`.
- Validate compile/lints before cross-crate changes: `rustup run stable cargo clippy -p crosspack-registry --all-targets -- -D warnings`.
- When touching trust/snapshot flow, also run workspace tests: `rustup run stable cargo test --workspace`.
