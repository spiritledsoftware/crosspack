# CRATES KNOWLEDGE BASE

## OVERVIEW
`crates/` is the workspace domain boundary: one CLI crate orchestrates five library crates for manifest model, registry trust, dependency resolution, install lifecycle, and cryptographic verification.

## STRUCTURE
- `crates/crosspack-cli/` - command routing and user-visible lifecycle output; complexity hotspot: `crates/crosspack-cli/src/main.rs` command match + install/upgrade orchestration helpers.
- `crates/crosspack-core/` - shared manifest schema/types; hotspot: `crates/crosspack-core/src/lib.rs` `Artifact::archive_type` + TOML decode contract.
- `crates/crosspack-installer/` - filesystem lifecycle and state receipts; hotspots: `crates/crosspack-installer/src/lib.rs` uninstall reachability/pruning and extraction/strip pipeline.
- `crates/crosspack-registry/` - registry index traversal + signature-checked manifest loading; hotspot: `crates/crosspack-registry/src/lib.rs` `package_versions` verification + parse path.
- `crates/crosspack-resolver/` - backtracking dependency solver and install ordering; hotspots: `crates/crosspack-resolver/src/lib.rs` `search` recursion + `topo_order` cycle detection.
- `crates/crosspack-security/` - checksum/signature primitives used by CLI/registry; hotspot: `crates/crosspack-security/src/lib.rs` `verify_ed25519_signature_hex` decode/length checks.

## WHERE TO LOOK
- Add/change CLI subcommands, stdout wording, install/upgrade flow: `crates/crosspack-cli/src/main.rs` (`main`, `resolve_install_graph`, `install_resolved`, `build_upgrade_plans`).
- Change manifest fields or artifact metadata parsing: `crates/crosspack-core/src/lib.rs` (`PackageManifest`, `Artifact`, `ArchiveType`).
- Change install/uninstall state transitions or receipt format: `crates/crosspack-installer/src/lib.rs` (`write_install_receipt`, `parse_receipt`, `uninstall_package`).
- Change package search/info loading behavior: `crates/crosspack-registry/src/lib.rs` (`search_names`, `package_versions`).
- Change dependency or pin selection semantics: `crates/crosspack-resolver/src/lib.rs` (`resolve_dependency_graph`, `matching_candidates`, `selected_satisfies_constraints`).
- Change hash/signature verification behavior: `crates/crosspack-security/src/lib.rs` (`verify_sha256_file`, `verify_ed25519_signature_hex`).
- Existing crate-local guidance: `crates/crosspack-cli/AGENTS.md`, `crates/crosspack-installer/AGENTS.md`, `crates/crosspack-resolver/AGENTS.md`.

## CONVENTIONS
- Keep orchestration and all user-facing text in `crates/crosspack-cli`; library crates return data/errors, not CLI output.
- Keep shared model definitions in `crates/crosspack-core`; do not duplicate manifest structs in consumer crates.
- Use crate boundaries as ownership boundaries: registry verifies and parses manifests, resolver selects versions, installer mutates prefix state.
- Preserve deterministic behavior across crates: sorted package names/versions, stable dependency order, stable lifecycle wording.
- Place behavior tests next to owning crate code (single-file crates use `#[cfg(test)]` in `src/lib.rs` or `src/main.rs`).
- Prefer extending existing hotspot helpers before adding parallel code paths that bypass crate contracts.

## ANTI-PATTERNS
- Moving domain logic from libraries into `crates/crosspack-cli/src/main.rs` because it is "already there".
- Introducing cross-crate duplicate types for manifests/receipts/dependency roots instead of reusing `crosspack-core` and installer structs.
- Bypassing registry signature verification (`package_versions`) when adding new index read paths.
- Bypassing resolver backtracking path (`search`/`matching_candidates`) with ad-hoc highest-version selection in callers.
- Mutating prefix filesystem state outside `crates/crosspack-installer/src/lib.rs` APIs.
- Making security checks optional or silent; verification failures must remain explicit errors.
- Editing crate behavior without updating that crate's tests in the same file/module.
