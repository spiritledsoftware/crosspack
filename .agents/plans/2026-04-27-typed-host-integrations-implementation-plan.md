# Typed Host Integrations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use test-driven-development before production code. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a minimal declarative `[[integrations]]` manifest feature for Docker CLI plugins, PATH plugins, and staged service metadata.

**Architecture:** `crosspack-core` owns typed serde models and validation. `crosspack-installer` owns Crosspack-managed projection paths and `.integrations` sidecar state. `crosspack-cli` wires projection into the existing install flow and cleanup into uninstall through installer APIs.

**Tech Stack:** Rust workspace, serde TOML parsing, existing installer filesystem/state helpers.

---

### Task 1: Core Manifest Schema

**Files:**
- Modify: `crates/crosspack-core/src/manifest.rs`
- Modify: `crates/crosspack-core/src/lib.rs`
- Test: `crates/crosspack-core/src/tests.rs`

- [ ] Write failing tests for parsing all three integration kinds, rejecting duplicate integration ownership, and rejecting unsafe source paths.
- [ ] Run `cargo test -p crosspack-core manifest_integration -- --nocapture` and confirm failures are for missing schema.
- [ ] Add `PackageIntegration` enum and validation helpers.
- [ ] Export the new types from `crosspack-core`.
- [ ] Re-run focused tests until green.

### Task 2: Installer Projection and State

**Files:**
- Modify: `crates/crosspack-installer/src/layout.rs`
- Modify: `crates/crosspack-installer/src/types.rs`
- Modify: `crates/crosspack-installer/src/exposure.rs`
- Modify: `crates/crosspack-installer/src/lib.rs`
- Modify: `crates/crosspack-installer/src/uninstall.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] Write failing tests for projecting Docker CLI plugin, PATH plugin, and service metadata under `share/integrations`.
- [ ] Write failing tests for `.integrations` state round-trip and removal on uninstall helpers.
- [ ] Run `cargo test -p crosspack-installer integration -- --nocapture` and confirm failures are for missing APIs.
- [ ] Add layout paths for integration root and state file.
- [ ] Add projection record type and read/write/clear functions.
- [ ] Add `expose_integration`, `remove_exposed_integration`, and stale cleanup helpers.
- [ ] Re-run focused tests until green.

### Task 3: CLI Install Wiring

**Files:**
- Modify: `crates/crosspack-cli/src/main.rs`
- Modify: `crates/crosspack-cli/src/core_flows.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] Write failing CLI test proving install projects declared integration and records state.
- [ ] Run the focused CLI test and confirm it fails for missing install wiring.
- [ ] Import installer integration APIs.
- [ ] Project `resolved.manifest.integrations` after binaries/completions/GUI exposure.
- [ ] Remove stale integration projections on reinstall.
- [ ] Persist `.integrations` state next to receipt write.
- [ ] Re-run focused test until green.

### Task 4: Registry Package Coverage

**Files:**
- Modify/create registry package and release TOML files for `docker-compose`, `kubectx`, and one service package if registry manifests can safely stage service source.

- [ ] Inspect existing registry schema support for unknown manifest keys.
- [ ] Add one package of each integration class only if registry validation accepts/will emit `[[integrations]]`.
- [ ] Run registry validation commands.

### Task 5: Verification

- [ ] Run `cargo fmt --all --check`.
- [ ] Run `cargo test -p crosspack-core`.
- [ ] Run `cargo test -p crosspack-installer`.
- [ ] Run `cargo test -p crosspack-cli` or focused equivalent if full suite is too slow.
- [ ] Run `cargo clippy --workspace --all-targets --all-features -- -D warnings` if time permits.
