---
title: Installed Identity Profile Model
summary: Identity-keyed concurrent installs use profile/target/namespace/provenance/package identity, with legacy name-keyed compatibility retained for migration and rollback visibility.
tags: []
related: [architecture/installed_identity_profile_model/legacy_state_compatibility_in_installed_package_state_reader.md, architecture/host_integrations/typed_host_integrations.overview.md, architecture/installer_state_model/installed_identity_profile_model.overview.md]
keywords: []
createdAt: '2026-04-29T17:28:11.494Z'
updatedAt: '2026-05-01T09:39:13.877Z'
---
## Reason
Capture the implemented identity-keyed concurrent install model and lifecycle behavior

## Raw Concept
**Task:**
Document the completed installed identity profile model work for concurrent installs and lifecycle operations.

**Changes:**
- Rejected a flat <identity-key> layout as too naive for storage identity
- Added source namespace and source provenance to InstalledPackageIdentity
- Added InstalledPackageSelector with dimension-aware matching
- Added selector and label helpers for identity display and state key generation
- Preserved compatibility for older identity state documents
- Added fallback reads from new identity state path, legacy identity state path, and name-keyed state path
- Validated the implementation with installer tests and clippy
- Added identity layout path coverage and ensured identity_pkgs_dir() is created by ensure_base_dirs()
- Added/re-exported identity receipt APIs, including parse_identity_receipt
- Preserved legacy receipt parsing and added identity receipt hydration coverage
- Removed duplicate test definitions in tests.rs
- Added identity-aware installer/core support and CLI routing
- Implemented identity-keyed receipt, state, layout, exposure, native, pin, artifact, and uninstall logic
- Updated shipped docs and test coverage for identity-aware behavior

**Files:**
- .agents/specs/installed-identity-profile-model-spec.md
- .agents/plans/2026-04-29-installed-identity-profile-model-implementation-plan.md
- docs/architecture.md
- docs/install-flow.md
- crates/crosspack-installer/src/identity.rs
- crates/crosspack-installer/src/installed_state.rs
- crates/crosspack-installer/src/layout.rs
- crates/crosspack-installer/src/receipts.rs
- crates/crosspack-installer/src/exposure.rs
- crates/crosspack-installer/src/native.rs
- crates/crosspack-installer/src/pins.rs
- crates/crosspack-installer/src/uninstall.rs
- crates/crosspack-installer/src/artifact.rs
- crates/crosspack-installer/src/lib.rs
- crates/crosspack-cli/src/main.rs
- crates/crosspack-cli/src/core_flows.rs
- crates/crosspack-cli/src/command_flows.rs
- crates/crosspack-cli/src/dispatch.rs
- crates/crosspack-cli/src/tests.rs
- crates/crosspack-resolver/src/plan.rs

**Flow:**
resolve selector -> derive identity -> install into identity-scoped layout -> record receipts/state -> expose integrations -> resolve lifecycle actions by identity

**Timestamp:** 2026-05-01T09:39:04.566Z

**Author:** ByteRover context engineer

## Narrative
### Structure
The work spans spec and implementation artifacts across installer, CLI, docs, and tests. Identity-scoped storage prevents same-name packages from overwriting each other while preserving legacy compatibility paths for migration and rollback.

### Dependencies
Depends on typed install plan modeling, selector resolution, and identity-scoped uninstall and rollback handling. Bare-name lifecycle actions now require disambiguation guidance when multiple identities match.

### Highlights
PR #112 was merged after review fixes, including identity-keyed uninstall dependency graph handling, native uninstall cleanup, and writing legacy package-keyed receipts alongside identity receipts for rollback visibility.

### Rules
No post-install scripts; prefer declarative, deterministic installs and typed host integrations. Do not manually bump the root repo registry submodule. Same-name concurrent installs must be handled with identity-keyed storage, not selector-only fixes.

### Examples
Validation sequence completed successfully: cargo fmt --all --check, cargo clippy --workspace --all-targets --all-features -- -D warnings, cargo build --workspace --locked, cargo test --workspace, git diff --check, and scripts/validate-snapshot-flow.sh.

## Facts
- **installed_package_identity_fields**: InstalledPackageIdentity includes profile, target, source_namespace, source_provenance, and package. [project]
- **identity_keyed_storage_path**: New install storage is identity-keyed under pkgs/identities/v1/<profile>/<target>/<namespace>/<package>/<version>/. [project]
- **selector_compatibility**: Identity selectors are required for disambiguation, but legacy name-keyed reads remain supported. [project]
- **ambiguity_policy**: Fail closed on duplicate installed identities and ambiguous bare-name lifecycle actions. [project]
- **legacy_compatibility**: Legacy package-keyed receipts and state remain visible only for migration and rollback compatibility. [project]
- **validation_suite**: Validation passed with cargo fmt, cargo clippy, cargo build, cargo test, git diff --check, and scripts/validate-snapshot-flow.sh. [project]
