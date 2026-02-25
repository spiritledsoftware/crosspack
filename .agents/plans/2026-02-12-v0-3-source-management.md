# v0.3 Source Management Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement full v0.3 source management with `git` and `filesystem` sources, trust pinning, snapshot updates, and deterministic multi-source metadata reads.

**Architecture:** Keep single-root `RegistryIndex` behavior intact for explicit `--registry_root`. Add a source-state and snapshot layer in `crosspack-registry` for configured sources and prioritized reads. Keep command wiring and user-facing output in `crosspack-cli`.

**Tech Stack:** Rust workspace crates, `clap`, `anyhow`, `serde`, `toml`, `serde_json`, `sha2`, existing `ed25519` verification, deterministic sort/order semantics.

---

### Task 0: Isolated workspace and baseline verification

**Files:**
- Create: `.worktrees/feature-v0-3-source-management/` (via git worktree)

**Step 1: Create worktree**

Run: `git worktree add .worktrees/feature-v0-3-source-management -b feature/v0-3-source-management`
Expected: new isolated branch and worktree.

**Step 2: Baseline tests**

Run: `cargo test -p crosspack-registry && cargo test -p crosspack-cli`
Expected: PASS baseline before edits.

### Task 1: Source config state model and persistence API

**Files:**
- Modify: `crates/crosspack-registry/Cargo.toml`
- Modify: `crates/crosspack-registry/src/lib.rs`
- Test: `crates/crosspack-registry/src/lib.rs`

**Step 1: Write failing tests**

Add tests:
- `source_store_add_rejects_duplicate_name`
- `source_store_add_rejects_invalid_name`
- `source_store_add_rejects_invalid_fingerprint`
- `source_store_list_sorts_by_priority_then_name`
- `source_store_remove_reports_missing_source`

**Step 2: Run targeted test and verify failure**

Run: `cargo test -p crosspack-registry source_store_add_rejects_duplicate_name -- --exact`
Expected: FAIL because source-store APIs are not implemented yet.

**Step 3: Implement minimal source-store API**

Add source configuration structs and store API in `crates/crosspack-registry/src/lib.rs`:
- `RegistrySourceKind` (`git`, `filesystem`)
- `RegistrySourceRecord`
- `RegistrySourceStore` with:
  - `add_source(...)`
  - `list_sources(...)`
  - `remove_source(...)`

Persist to `<state-root>/sources.toml` with deterministic ordering `(priority, name)`.

**Step 4: Run crate tests**

Run: `cargo test -p crosspack-registry`
Expected: PASS for new source-state tests and existing tests.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-registry/Cargo.toml crates/crosspack-registry/src/lib.rs
git commit -m "feat(registry): add source configuration state management"
```

### Task 2: Filesystem source update pipeline with trust pinning

**Files:**
- Modify: `crates/crosspack-registry/src/lib.rs`
- Test: `crates/crosspack-registry/src/lib.rs`

**Step 1: Write failing tests**

Add tests:
- `update_filesystem_source_writes_ready_snapshot`
- `update_filesystem_source_fails_on_fingerprint_mismatch`
- `update_filesystem_source_preserves_existing_cache_on_failure`
- `update_unknown_source_returns_source_not_found`

**Step 2: Run targeted test and verify failure**

Run: `cargo test -p crosspack-registry update_filesystem_source_writes_ready_snapshot -- --exact`
Expected: FAIL before implementation.

**Step 3: Implement filesystem update behavior**

Add `update_sources` and per-source update result model.

For filesystem source:
1. Copy source into temp directory.
2. Validate required files:
   - `registry.pub`
   - `index/`
3. Compute key fingerprint from raw `registry.pub` bytes and compare with configured fingerprint.
4. Verify metadata signature policy can be enforced (use existing `RegistryIndex` verification path).
5. Atomically replace `<state-root>/cache/<name>/`.
6. Write `<state-root>/cache/<name>/snapshot.json`.

If any step fails, preserve prior cache unchanged.

**Step 4: Run crate tests**

Run: `cargo test -p crosspack-registry`
Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-registry/src/lib.rs
git commit -m "feat(registry): implement filesystem source update and snapshot verification"
```

### Task 3: Git source synchronization support

**Files:**
- Modify: `crates/crosspack-registry/src/lib.rs`
- Test: `crates/crosspack-registry/src/lib.rs`

**Step 1: Write failing tests**

Add tests:
- `update_git_source_clones_and_records_snapshot_id`
- `update_git_source_fetches_new_commit`
- `update_git_source_returns_up_to_date_when_revision_unchanged`

**Step 2: Run targeted test and verify failure**

Run: `cargo test -p crosspack-registry update_git_source_clones_and_records_snapshot_id -- --exact`
Expected: FAIL before implementation.

**Step 3: Implement git sync flow**

Add git command execution helper in `crosspack-registry`:
- clone into temp when cache missing,
- fetch and reset when cache exists,
- derive snapshot id via `git rev-parse --short=16 HEAD`.

Then run same trust and snapshot verification pipeline used by filesystem sources.

**Step 4: Run crate tests**

Run: `cargo test -p crosspack-registry`
Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-registry/src/lib.rs
git commit -m "feat(registry): add git source synchronization for updates"
```

### Task 4: Multi-source snapshot-backed metadata reads with precedence

**Files:**
- Modify: `crates/crosspack-registry/src/lib.rs`
- Test: `crates/crosspack-registry/src/lib.rs`

**Step 1: Write failing tests**

Add tests:
- `configured_index_package_versions_prefers_higher_priority_source`
- `configured_index_package_versions_uses_name_tiebreaker`
- `configured_index_search_names_deduplicates_across_sources`
- `configured_index_fails_when_no_ready_snapshot_exists`

**Step 2: Run targeted test and verify failure**

Run: `cargo test -p crosspack-registry configured_index_package_versions_prefers_higher_priority_source -- --exact`
Expected: FAIL before implementation.

**Step 3: Implement configured-index API**

Add `ConfiguredRegistryIndex` backed by `<state-root>`:
- `search_names(needle)`
- `package_versions(package)`

Read only ready snapshots and apply source precedence:
1. lower `priority` first,
2. lexicographically smaller source `name` as tie-break.

Keep fail-closed behavior for key/signature issues.

**Step 4: Run crate tests**

Run: `cargo test -p crosspack-registry`
Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-registry/src/lib.rs
git commit -m "feat(registry): read metadata from prioritized source snapshots"
```

### Task 5: CLI commands for registry management and update

**Files:**
- Modify: `crates/crosspack-cli/src/main.rs`
- Test: `crates/crosspack-cli/src/main.rs`

**Step 1: Write failing tests**

Add tests:
- `cli_parses_registry_add_command`
- `cli_parses_registry_remove_with_purge_cache`
- `cli_parses_update_with_multiple_registry_flags`
- `registry_list_output_is_sorted`

**Step 2: Run targeted test and verify failure**

Run: `cargo test -p crosspack-cli cli_parses_registry_add_command -- --exact`
Expected: FAIL before implementation.

**Step 3: Implement commands**

Extend clap command model in `crates/crosspack-cli/src/main.rs`:
- `registry add/list/remove`
- `update [--registry <name>]...`

Wire commands to registry store/update APIs.

Emit deterministic statuses and summary:
- per source: `updated`, `up-to-date`, `failed`
- summary: `update summary: updated=<n> up-to-date=<n> failed=<n>`

**Step 4: Run crate tests**

Run: `cargo test -p crosspack-cli`
Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-cli/src/main.rs
git commit -m "feat(cli): add registry source management and update commands"
```

### Task 6: Default metadata reads from configured snapshots

**Files:**
- Modify: `crates/crosspack-cli/src/main.rs`
- Test: `crates/crosspack-cli/src/main.rs`

**Step 1: Write failing tests**

Add tests:
- `search_uses_registry_root_override_when_present`
- `search_uses_configured_sources_without_registry_root`
- `metadata_commands_fail_with_guidance_when_no_sources_or_snapshots`

**Step 2: Run targeted test and verify failure**

Run: `cargo test -p crosspack-cli search_uses_configured_sources_without_registry_root -- --exact`
Expected: FAIL before implementation.

**Step 3: Implement backend selection**

Add CLI helper that selects registry backend:
- if `--registry_root` is present: use existing `RegistryIndex`.
- otherwise: use `ConfiguredRegistryIndex` at `<prefix>/state/registries`.

Route `search`, `info`, `install`, and `upgrade` through a shared backend abstraction with methods matching current usage (`search_names`, `package_versions`).

**Step 4: Run crate tests**

Run: `cargo test -p crosspack-cli`
Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-cli/src/main.rs
git commit -m "feat(cli): use configured snapshots for metadata by default"
```

### Task 7: Documentation synchronization

**Files:**
- Modify: `docs/architecture.md`
- Modify: `docs/install-flow.md`
- Modify: `docs/registry-spec.md`
- Optional modify: `docs/manifest-spec.md`

**Step 1: Update docs for implemented behavior**

Update core docs to describe implemented v0.3 source-management workflow and `--registry_root` override behavior.

**Step 2: Verify CLI surface**

Run: `cargo run -p crosspack-cli -- --help`
Expected: command list includes `registry` and `update`.

**Step 3: Commit**

Run:
```bash
git add docs/architecture.md docs/install-flow.md docs/registry-spec.md docs/manifest-spec.md
git commit -m "docs: document implemented source management workflow"
```

### Task 8: Final verification

**Files:** none

**Step 1: Format check**

Run: `cargo fmt --all --check`
Expected: PASS.

**Step 2: Lint check**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: PASS.

**Step 3: Full test suite**

Run: `cargo test --workspace`
Expected: PASS.

**Step 4: Commit sanity**

Run: `git log --oneline -8`
Expected: includes v0.3 task commits in order.
